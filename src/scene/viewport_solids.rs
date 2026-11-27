use super::*;

impl Scene {
    /// True for the modeler solid families whose wire path is an empty shell
    /// (their geometry lives in the mesh pipeline): B-rep corners for those
    /// come from the mesh edge list instead of wire points.
    pub(in crate::scene) fn is_solid_shell_entity(entity: &EntityType) -> bool {
        matches!(
            entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
        )
    }

    /// B-rep snap points for a solid: edge endpoints as `Vertex` hints and
    /// edge centres as `EdgeMidpoint` hints (both from the tessellated edge
    /// list), plus face centres as `FaceCenter` hints (from the kernel B-rep
    /// faces). Empty for non-solids and for solids with no usable data.
    pub(crate) fn solid_snap_points(
        &self,
        handle: Handle,
    ) -> Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)> {
        use crate::scene::model::wire_model::SnapHint;
        let Some(entity) = self.document.get_entity(handle) else {
            return Vec::new();
        };
        if !Self::is_solid_shell_entity(entity) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(set) = self
            .meshes
            .get(&handle)
            .or_else(|| self.block_meshes.get(&handle))
        {
            let (edges, lows) = set.geometry_edges();
            let mut seen = HashSet::default();
            for (a, b) in crate::scene::model::mesh_model::solid_edge_segments(edges, lows) {
                for (point, hint) in [(a, SnapHint::Vertex), (b, SnapHint::Vertex)] {
                    let key = [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
                    if seen.insert(key) {
                        out.push((point, hint));
                    }
                }
                out.push(((a + b) * 0.5, SnapHint::EdgeMidpoint));
            }
        }
        out.extend(
            Self::solid_face_centers(entity, self.solid_models.get(&handle))
                .into_iter()
                .map(|p| (p, SnapHint::FaceCenter)),
        );
        out
    }

    /// Face centres for a solid entity: kernel B-rep faces averaged over
    /// their boundary loops. Prefers the cached kernel body (no re-parse);
    /// falls back to parsing the entity's ACIS data, which shares the cost
    /// profile of the mesh build at the same epoch. No body ⇒ no centres.
    pub(in crate::scene) fn solid_face_centers(
        entity: &EntityType,
        cached: Option<&ocs_solid::model::Brep>,
    ) -> Vec<glam::DVec3> {
        use crate::scene::convert::solid3d_tess::{region_record, solid_record};
        use crate::scene::model::solid_model::face_centers;
        if let Some(body) = cached {
            return face_centers(body)
                .into_iter()
                .map(glam::DVec3::from_array)
                .collect();
        }
        match entity {
            EntityType::Solid3D(s) => solid_record(s),
            EntityType::Region(r) => region_record(r),
            _ => None,
        }
        .map(|body| face_centers(&body))
        .unwrap_or_default()
        .into_iter()
        .map(glam::DVec3::from_array)
        .collect()
    }

    /// Attach solid B-rep snaps to the entity's shell wire (the empty wire
    /// carrying only the insertion snap). History/construction wires keep
    /// their own points and are left alone; non-solids are untouched.
    /// Idempotent: a wire already carrying 3D snaps is skipped, so shared
    /// memo entries can pass through every assembly path safely.
    pub(crate) fn attach_solid_snaps(&self, handle: Handle, wires: &mut [WireModel]) {
        use crate::scene::model::wire_model::SnapHint;
        let points = self.solid_snap_points(handle);
        if points.is_empty() {
            return;
        }
        for wire in wires.iter_mut() {
            if !wire.points.is_empty() {
                continue;
            }
            if wire.snap_pts.iter().any(|(_, hint)| {
                matches!(
                    hint,
                    SnapHint::Vertex
                        | SnapHint::EdgeMidpoint
                        | SnapHint::FaceCenter
                        | SnapHint::Knot
                )
            }) {
                continue;
            }
            wire.snap_pts.extend(points.iter().copied());
        }
    }

    /// Live solid-face snaps for a command point: Nearest-to-face (screen
    /// nearest point on a mesh triangle) and Perpendicular-to-face (foot from
    /// `base` onto a triangle). Mesh triangles carry no B-rep face grouping,
    /// so faces are individual triangles here — the pre-baked `FaceCenter`
    /// hints (true B-rep faces) are the separate discrete counterpart.
    ///
    /// Top-level model meshes only: block-local sets live in a different
    /// space, and xclipped views are not honored (the wire channel the
    /// pre-baked 3D hints ride is clipped; this direct mesh read is not).
    /// Worst case in those views is a snap with no geometry behind it —
    /// never a corrupt point.
    pub fn solid_face_snaps(
        &self,
        cursor_screen: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        aperture_px: f32,
        base: Option<glam::DVec3>,
        want_perp: bool,
        want_nearest: bool,
    ) -> Option<crate::snap::SnapResult> {
        use crate::snap::{
            closest_point_on_tri_2d, foot_on_triangle, snap_better, snap_priority, snap_tier,
            SnapType,
        };
        if !want_perp && !want_nearest {
            return None;
        }
        if !(aperture_px.is_finite() && aperture_px > 0.0) {
            return None;
        }
        let perp_base = want_perp.then_some(base).flatten();
        // Eye-plane guard: a triangle behind the perspective eye mirrors onto
        // visible pixels and would otherwise win a snap for a hidden face (#64).
        let project = |world: glam::DVec3| -> Option<[f32; 2]> {
            let ndc = crate::scene::view::projection::project_ndc(view_rot, eye, world)?;
            Some([
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            ])
        };
        let in_bounds = |s: [f32; 2]| {
            s[0] >= 0.0 && s[0] <= bounds.width && s[1] >= 0.0 && s[1] <= bounds.height
        };
        let radius2 = aperture_px * aperture_px;
        let cursor = [cursor_screen.x, cursor_screen.y];
        // (tier, d2, sub, eye-depth², handle, hit): engine ordering, then
        // nearer-to-eye (coincident faces share pixels — the top face must
        // beat the bottom one), then the handle as a final deterministic
        // tie-break (mesh iteration order is not stable).
        let mut best: Option<(u8, f32, u8, f64, u64, crate::snap::SnapResult)> = None;
        let mut consider = |snap_type: SnapType,
                            world: glam::DVec3,
                            screen: [f32; 2],
                            d2: f32,
                            handle: Handle| {
            if !(d2 < radius2) || !in_bounds(screen) {
                return;
            }
            let (tier, sub) = (snap_tier(snap_type), snap_priority(snap_type));
            let depth2 = (world - eye).length_squared();
            let better = match &best {
                None => true,
                Some((bt, bd2, bs, bdepth2, bh, _)) => {
                    snap_better(tier, d2, sub, (*bt, *bd2, *bs))
                        || (tier == *bt
                            && (d2 - *bd2).abs() <= 1e-4
                            && sub == *bs
                            && (depth2 < *bdepth2 || (depth2 == *bdepth2 && handle.value() < *bh)))
                }
            };
            if better {
                best = Some((
                    tier,
                    d2,
                    sub,
                    depth2,
                    handle.value(),
                    crate::snap::SnapResult {
                        world,
                        screen: iced::Point::new(screen[0], screen[1]),
                        snap_type,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                        extension_origin: None,
                        extension_dir: None,
                        viewport: None,
                        source: Some(crate::snap::DimensionAssociationSource::inferred(handle)),
                        secondary_source: None,
                        model_point: None,
                    },
                ));
            }
        };
        let aperture_rect = [
            cursor[0] - aperture_px,
            cursor[1] - aperture_px,
            cursor[0] + aperture_px,
            cursor[1] + aperture_px,
        ];
        let frustum = crate::scene::pick::interaction_index::ScreenRectFrustum::new(
            aperture_rect,
            view_rot,
            eye,
            bounds,
        );
        for (handle, set) in self.meshes.iter() {
            let Some(lod) = set.geometry_lods().first() else {
                continue;
            };
            if lod.indices.len() < 3 {
                continue;
            }
            if !frustum.aabb_intersects(
                [
                    set.world_aabb[0] as f64,
                    set.world_aabb[1] as f64,
                    set.z_aabb[0] as f64,
                ],
                [
                    set.world_aabb[2] as f64,
                    set.world_aabb[3] as f64,
                    set.z_aabb[1] as f64,
                ],
            ) {
                continue;
            }
            let mut evaluate_triangle = |tri: &[u32]| {
                let mut v = [glam::DVec3::ZERO; 3];
                let mut ok = true;
                for (slot, index) in v.iter_mut().zip(tri.iter()) {
                    let (Some(high), low) = (
                        lod.verts.get(*index as usize),
                        lod.verts_low
                            .get(*index as usize)
                            .copied()
                            .unwrap_or([0.0; 3]),
                    ) else {
                        ok = false;
                        break;
                    };
                    *slot = glam::DVec3::new(
                        high[0] as f64 + low[0] as f64,
                        high[1] as f64 + low[1] as f64,
                        high[2] as f64 + low[2] as f64,
                    );
                    if !slot.is_finite() {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    return;
                }
                let [a, b, c] = v;
                let (Some(sa), Some(sb), Some(sc)) = (project(a), project(b), project(c)) else {
                    return;
                };
                // Per-triangle screen reject before any 3D work.
                let (tx0, ty0) = (sa[0].min(sb[0]).min(sc[0]), sa[1].min(sb[1]).min(sc[1]));
                let (tx1, ty1) = (sa[0].max(sb[0]).max(sc[0]), sa[1].max(sb[1]).max(sc[1]));
                if cursor[0] < tx0 - aperture_px
                    || cursor[0] > tx1 + aperture_px
                    || cursor[1] < ty0 - aperture_px
                    || cursor[1] > ty1 + aperture_px
                {
                    return;
                }
                if want_nearest {
                    let (q, d2, w) = closest_point_on_tri_2d(cursor, sa, sb, sc);
                    if d2 < radius2 {
                        let world = a * w[0] as f64 + b * w[1] as f64 + c * w[2] as f64;
                        consider(SnapType::NearestFace, world, q, d2, *handle);
                    }
                }
                if let Some(base3d) = perp_base {
                    if let Some(foot) = foot_on_triangle(base3d, a, b, c) {
                        if let Some(sf) = project(foot) {
                            let dx = sf[0] - cursor[0];
                            let dy = sf[1] - cursor[1];
                            consider(
                                SnapType::FacePerpendicular,
                                foot,
                                sf,
                                dx * dx + dy * dy,
                                *handle,
                            );
                        }
                    }
                }
            };
            if let Some(bvh) = set.geometry_triangle_bvh(0) {
                use parry3d_f64::partitioning::TraversalAction;

                bvh.traverse(|node| {
                    let mins = node.mins();
                    let maxs = node.maxs();
                    if !frustum.aabb_intersects(
                        [mins.x, mins.y, mins.z],
                        [maxs.x, maxs.y, maxs.z],
                    ) {
                        return TraversalAction::Prune;
                    }
                    if let Some(tri_idx) = node.leaf_data() {
                        let start = (tri_idx as usize) * 3;
                        if let Some(tri) = lod.indices.get(start..start + 3) {
                            evaluate_triangle(tri);
                        }
                    }
                    TraversalAction::Continue
                });
            } else {
                for tri in lod.indices.chunks_exact(3) {
                    evaluate_triangle(tri);
                }
            }
        }
        best.map(|(_, _, _, _, _, hit)| hit)
    }
}

impl Scene {

    /// Prepare display geometry without changing the entity or resident caches.
    pub(crate) fn prepare_solid_model_display(
        &self,
        handle: Handle,
        solid: &ocs_solid::model::Brep,
    ) -> Option<(MeshLodSet, Vec<document::entities::Wire>, [f64; 3])> {
        let color = self
            .document
            .get_entity(handle)
            .map(|e| self.render_style(e).0)
            .unwrap_or([0.8, 0.8, 0.85, 1.0]);
        let facet_resolution = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_resolution);
        let (isolines, planar_isolines) = match self.document.get_entity(handle) {
            Some(EntityType::Surface(surface)) => (
                crate::entities::solid3d::surface_isoline_counts(surface),
                Some(crate::entities::solid3d::surface_property_state(surface).isolines),
            ),
            _ => (
                [crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines); 2],
                None,
            ),
        };
        let (mut set, wires, center) = crate::scene::model::solid_model::display_from_solid(
            solid,
            color,
            facet_resolution,
            chordal_deflection,
            isolines,
            planar_isolines,
        )?;
        let name = handle.value().to_string();
        for mesh in &mut set.lods {
            mesh.name = name.clone();
        }
        if let Some(entity) = self.document.get_entity(handle) {
            crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                entity,
                color,
                None,
                self.material_base_dir.as_deref(),
            )
            .apply_to_with_face_overrides(
                &mut set,
                &self.document,
                self.material_base_dir.as_deref(),
            );
            crate::scene::model::visual_style_model::apply_mesh_visual_style(
                &mut set,
                &self.document,
                entity,
            );
        }
        Some((set, wires, center))
    }

    /// Commit a prepared display without tessellating the body a second time.
    pub(crate) fn register_prepared_solid_model(
        &mut self,
        handle: Handle,
        solid: ocs_solid::model::Brep,
        display: (MeshLodSet, Vec<document::entities::Wire>, [f64; 3]),
    ) -> bool {
        let (mut set, wires, center) = display;
        // New command results can be prepared before their handle exists.
        // Apply the committed entity's actual style without retessellating.
        if let Some(entity) = self.document.get_entity(handle) {
            let color = self.render_style(entity).0;
            for mesh in &mut set.lods {
                mesh.name = handle.value().to_string();
                mesh.color = color;
            }
            crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                entity,
                color,
                None,
                self.material_base_dir.as_deref(),
            )
            .apply_to_with_face_overrides(
                &mut set,
                &self.document,
                self.material_base_dir.as_deref(),
            );
            crate::scene::model::visual_style_model::apply_mesh_visual_style(
                &mut set,
                &self.document,
                entity,
            );
        }
        if let Some(entity) = self.document.get_entity_mut(handle) {
            let reference = document::types::Vector3::new(center[0], center[1], center[2]);
            match entity {
                EntityType::Solid3D(entity) => {
                    entity.point_of_reference = reference;
                    entity.wires = wires;
                    entity.silhouettes.clear();
                }
                EntityType::Surface(entity) => {
                    entity.point_of_reference = reference;
                    entity.wires = wires;
                    entity.silhouettes.clear();
                }
                _ => {}
            }
        }
        self.meshes.insert(handle, set);
        self.solid_models.insert(handle, solid);
        self.sync_solid_reference_point(handle);
        self.bump_entities(&[(handle, ChangeKind::Modified)]);
        true
    }

    /// Cache and display a Model-tab kernel solid. A failed preparation leaves
    /// the previous entity, body and mesh untouched.
    pub fn register_solid_model(&mut self, handle: Handle, solid: ocs_solid::model::Brep) -> bool {
        let Some(display) = self.prepare_solid_model_display(handle, &solid) else {
            return false;
        };
        if !display.0.complete
            && matches!(
                self.document.solid_history_operation(handle),
                Some(document::objects::SolidHistoryOperation::Loft(_))
            )
        {
            return false;
        }
        self.register_prepared_solid_model(handle, solid, display)
    }

    /// `BACKGROUND` change picks up the new `adapt_to_bg` result without
    /// re-tessellating ACIS geometry. Caller must bump `geometry_epoch`
    /// afterwards so the GPU re-uploads the now-updated colour data.
    pub fn recolor_meshes(&mut self) {
        // Cache colour lookups by handle to avoid borrowing the document
        // re-entrantly through `render_style` inside a `&mut self` loop.
        // Covers both top-level solid meshes and block-definition meshes
        // (instanced per INSERT), so a solid recolours wherever it lives.
        // During REFEDIT, solids outside the edited set render faded.
        let bg = self.bg_color;
        let materials: HashMap<Handle, crate::scene::model::material_model::MeshMaterial> = self
            .meshes
            .keys()
            .chain(self.block_meshes.keys())
            .filter_map(|&h| {
                self.document.get_entity(h).map(|e| {
                    let color = self.render_style(e).0;
                    let mut material =
                        crate::scene::model::material_model::resolve_material_with_base(
                            &self.document,
                            e,
                            color,
                            None,
                            self.material_base_dir.as_deref(),
                        );
                    if let Some(keep) = &self.refedit_keep {
                        if !keep.contains(&h) {
                            material.diffuse = crate::scene::cache::block_cache::refedit_fade(
                                material.diffuse,
                                bg,
                            );
                        }
                    }
                    if self.meshes.contains_key(&h) {
                        material.diffuse = self.count_color(h, material.diffuse, bg);
                    }
                    (h, material)
                })
            })
            .collect();
        for (h, set) in self.meshes.iter_mut().chain(self.block_meshes.iter_mut()) {
            if let Some(material) = materials.get(h) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
            }
            if let Some(entity) = self.document.get_entity(*h) {
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    set,
                    &self.document,
                    entity,
                );
            }
            crate::scene::analysis::apply(set);
        }
    }

    /// Re-tessellates the shaded models and recolours them: the surface analysis turned on or
    /// off, or its options changed.
    pub fn invalidate_solid_meshes(&mut self) {
        self.populate_meshes_from_document();
        self.recolor_meshes();
        self.bump_geometry();
    }

    /// Shows the control net of the NURBS surface `handle` (CVSHOW): its rows and columns of
    /// control vertices drawn over the model until hidden.
    pub fn show_control_net(&mut self, handle: Handle, net: &solidcore_ops::surface_cvs::Net) {
        let mut wires = Vec::new();
        let at = |i: usize, j: usize| net.points[i * net.cols + j];
        let line = |points: Vec<[f64; 3]>, k: usize| {
            let mut wire = WireModel::solid_f64(
                format!("cv-net:{}:{k}", handle.value()),
                points,
                WireModel::HOVER,
                false,
            );
            wire.set_fixed_screen_width(1.5);
            wire
        };
        for i in 0..net.rows {
            wires.push(line((0..net.cols).map(|j| at(i, j)).collect(), wires.len()));
        }
        for j in 0..net.cols {
            wires.push(line((0..net.rows).map(|i| at(i, j)).collect(), wires.len()));
        }
        self.cv_net_wires.insert(handle, wires);
    }

    /// Hides every control net shown (CVHIDE).
    pub fn hide_control_nets(&mut self) {
        self.cv_net_wires.clear();
    }

    /// Whether the control net of `handle` is shown.
    pub fn control_net_shown(&self, handle: Handle) -> bool {
        self.cv_net_wires.contains_key(&handle)
    }

    /// Recolour only the named cached solids after a property edit.
    pub fn recolor_meshes_for_handles(&mut self, handles: &[Handle]) {
        let bg = self.bg_color;
        let materials: HashMap<Handle, crate::scene::model::material_model::MeshMaterial> = handles
            .iter()
            .filter(|handle| {
                self.meshes.contains_key(handle) || self.block_meshes.contains_key(handle)
            })
            .filter_map(|&handle| {
                self.document.get_entity(handle).map(|entity| {
                    let color = self.render_style(entity).0;
                    let mut material =
                        crate::scene::model::material_model::resolve_material_with_base(
                            &self.document,
                            entity,
                            color,
                            None,
                            self.material_base_dir.as_deref(),
                        );
                    if self
                        .refedit_keep
                        .as_ref()
                        .is_some_and(|keep| !keep.contains(&handle))
                    {
                        material.diffuse =
                            crate::scene::cache::block_cache::refedit_fade(material.diffuse, bg);
                    }
                    if self.meshes.contains_key(&handle) {
                        material.diffuse = self.count_color(handle, material.diffuse, bg);
                    }
                    (handle, material)
                })
            })
            .collect();
        for handle in handles {
            let Some(material) = materials.get(handle) else {
                continue;
            };
            if let Some(set) = self.meshes.get_mut(handle) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                if let Some(entity) = self.document.get_entity(*handle) {
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        set,
                        &self.document,
                        entity,
                    );
                }
                crate::scene::analysis::apply(set);
            } else if let Some(set) = self.block_meshes.get_mut(handle) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                if let Some(entity) = self.document.get_entity(*handle) {
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        set,
                        &self.document,
                        entity,
                    );
                }
            }
        }
    }

    /// Enter / leave the REFEDIT fade. `keep` holds the edited entities (left
    /// bright); everything else renders faded. Re-tessellates wires and
    /// recolours solids so the change shows immediately. (#136)
    pub fn set_refedit_keep(&mut self, keep: Option<HashSet<Handle>>) {
        self.refedit_keep = keep;
        self.recolor_meshes();
        // Fading is applied after the raw per-entity tessellation memo is read;
        // keep those expensive wires and only rebuild the resident assembly.
        self.bump_geometry_no_blocks();
    }

    /// Enter / leave the COUNT colouring; a no-op when nothing changed.
    /// Entering or leaving fades or restores the whole drawing; a change
    /// inside count mode recolours only the objects whose colour changed.
    pub fn set_count_display(&mut self, display: Option<CountDisplay>) {
        if self.count_display == display {
            return;
        }
        let old = std::mem::replace(&mut self.count_display, display);
        let (Some(old), Some(new)) = (old, self.count_display.as_ref()) else {
            self.caches.hatch_cache.borrow_mut().clear();
            self.recolor_meshes();
            self.bump_geometry_no_blocks();
            return;
        };
        let recoloured = (old.color, old.error_color) != (new.color, new.error_color);
        let class = |d: &CountDisplay, h: &Handle| (d.errors.contains(h), d.counted.contains(h));
        let changed: Vec<Handle> = old
            .counted
            .iter()
            .chain(&old.errors)
            .chain(&new.counted)
            .chain(&new.errors)
            .copied()
            .collect::<HashSet<Handle>>()
            .into_iter()
            .filter(|h| recoloured || class(&old, h) != class(new, h))
            .collect();
        self.recolor_meshes_for_handles(&changed);
        let changes: Vec<(Handle, ChangeKind)> = changed
            .into_iter()
            .map(|h| (h, ChangeKind::Modified))
            .collect();
        self.bump_entities_restyled(&changes);
    }

    /// The colour count mode gives an object drawn as part of `handle` (a
    /// top-level entity): the count colours, else faded; unchanged outside
    /// count mode.
    pub(crate) fn count_color(&self, handle: Handle, color: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
        let color = if self.visibility_revealed(handle) {
            crate::scene::cache::block_cache::fade_toward_bg(color, bg)
        } else {
            color
        };
        let Some(count) = &self.count_display else {
            return color;
        };
        let [r, g, b, _] = if count.errors.contains(&handle) {
            count.error_color
        } else if count.counted.contains(&handle) {
            count.color
        } else {
            return crate::scene::cache::block_cache::fade_toward_bg(color, bg);
        };
        [r, g, b, color[3]]
    }

    /// Fade the colours of wires that belong to entities outside the REFEDIT
    /// keep set (no-op when not editing). The geometry is untouched, so
    /// hit-testing still works on faded entities. In count mode the counted
    /// references and the duplicates take the count colours instead.
    pub(in crate::scene) fn apply_refedit_fade(&self, wires: &mut Vec<WireModel>, bg: [f32; 4]) {
        // Block copies kept as stubs draw in their prototype's colour; while
        // copies are recoloured one by one they are expanded instead.
        if (self.count_display.is_some()
            || self.refedit_keep.is_some()
            || self.block_visibility_reveal)
            && wires.iter().any(|wire| wire.stub.is_some())
        {
            *wires = crate::scene::model::instance_model::expand_stubs(wires).into_owned();
        }
        if let Some(count) = &self.count_display {
            for w in wires.iter_mut() {
                let (color, class) = match Self::handle_from_wire_name(&w.name) {
                    Some(h) if count.errors.contains(&h) => (count.error_color, 2u64),
                    Some(h) if count.counted.contains(&h) => (count.color, 1),
                    _ => (
                        crate::scene::cache::block_cache::fade_toward_bg(w.color, bg),
                        0,
                    ),
                };
                w.color = color;
                // Instances of one definition draw in one colour: counted and
                // duplicate references get instance groups of their own.
                // ponytail: tags the two top bits of the source id, which the
                // id counter never reaches.
                if let Some(instance) = w.render_instance.as_mut() {
                    instance.source_id |= class << 62;
                }
            }
        }
        if self.block_visibility_reveal {
            for wire in wires.iter_mut() {
                if Self::handle_from_wire_name(&wire.name)
                    .is_some_and(|handle| self.visibility_revealed(handle))
                {
                    wire.color = crate::scene::cache::block_cache::fade_toward_bg(wire.color, bg);
                    for vertex in &mut wire.text_verts {
                        vertex.color =
                            crate::scene::cache::block_cache::fade_toward_bg(vertex.color, bg);
                    }
                }
            }
        }
        let Some(keep) = &self.refedit_keep else {
            return;
        };
        for w in wires.iter_mut() {
            let handle = Self::handle_from_wire_name(&w.name);
            // A DWF underlay's page stays as it is outside the working set,
            // like an image or a PDF page; only its frame fades. A DGN
            // underlay's content fades with everything else.
            let dwf_page = w.is_display_only()
                && handle.is_some_and(|h| {
                    matches!(
                        self.document.get_entity(h),
                        Some(EntityType::Underlay(u))
                            if u.underlay_type == document::entities::UnderlayType::Dwf
                    )
                });
            let keep_bright = dwf_page || handle.is_some_and(|h| keep.contains(&h));
            if !keep_bright {
                w.color = crate::scene::cache::block_cache::refedit_fade(w.color, bg);
            }
        }
    }

    /// Switch the active layout without pretending the document geometry
    /// changed. Resident wire sets already key on block/background, so keeping
    /// `geometry_epoch` stable lets Model and Paper retain their CPU/GPU data
    /// across tab switches. Only caches whose contents genuinely depend on the
    /// active layout or its background are dropped.
    pub fn set_current_layout(&mut self, name: String) {
        if self.current_layout != name {
            self.persist_current_layout_state();
            self.current_layout = name;
            self.load_current_layout_state();
            self.sync_active_space_to_document();
            self.recolor_meshes();
            *self.caches.wire_cache.borrow_mut() = None;
            *self.caches.interaction_mesh_cache.borrow_mut() = None;
            *self.caches.mesh_pick_lookup_cache.borrow_mut() = None;
            *self.caches.mesh_pick_boxes_cache.borrow_mut() = None;
            *self.caches.insert_hatch_cache.borrow_mut() = None;
        }
    }

    pub(crate) fn load_layout_header_fields(&mut self) {
        if self.current_layout == "Model" {
            return;
        }
        if let Some((flags, insertion_base, min_extents, max_extents, min_limits, max_limits)) =
            self.document.objects.values().find_map(|object| {
                let ObjectType::Layout(layout) = object else {
                    return None;
                };
                (layout.name == self.current_layout).then_some((
                    layout.flags,
                    layout.insertion_base,
                    layout.min_extents,
                    layout.max_extents,
                    layout.min_limits,
                    layout.max_limits,
                ))
            })
        {
            self.document.header.paper_space_linetype_scaling = flags & 1 != 0;
            self.document.header.paper_space_limit_check = flags & 2 != 0;
            self.document.header.paper_space_insertion_base =
                document::types::Vector3::new(insertion_base.0, insertion_base.1, insertion_base.2);
            self.document.header.paper_space_extents_min =
                document::types::Vector3::new(min_extents.0, min_extents.1, min_extents.2);
            self.document.header.paper_space_extents_max =
                document::types::Vector3::new(max_extents.0, max_extents.1, max_extents.2);
            self.document.header.paper_space_limits_min =
                document::types::Vector2::new(min_limits.0, min_limits.1);
            self.document.header.paper_space_limits_max =
                document::types::Vector2::new(max_limits.0, max_limits.1);
        }
    }

    pub fn load_current_layout_state(&mut self) {
        self.caches.display_plot_style_cache
            .borrow_mut()
            .retain(|(layout, _), _| layout != &self.current_layout);
        self.caches.styled_wire_cache.borrow_mut().clear();
        self.caches.styled_hatch_cache.borrow_mut().clear();
        self.caches.styled_wire_fill_cache.borrow_mut().clear();
        self.load_layout_header_fields();
    }

    pub fn persist_current_layout_state(&mut self) {
        if self.current_layout == "Model" {
            return;
        }
        let header = &self.document.header;
        let psltscale = header.paper_space_linetype_scaling;
        let plimcheck = header.paper_space_limit_check;
        let insertion_base = header.paper_space_insertion_base;
        let min_extents = header.paper_space_extents_min;
        let max_extents = header.paper_space_extents_max;
        let min_limits = header.paper_space_limits_min;
        let max_limits = header.paper_space_limits_max;
        for object in self.document.objects.values_mut() {
            let ObjectType::Layout(layout) = object else {
                continue;
            };
            if layout.name == self.current_layout {
                layout.flags = if psltscale {
                    layout.flags | 1
                } else {
                    layout.flags & !1
                };
                layout.flags = if plimcheck {
                    layout.flags | 2
                } else {
                    layout.flags & !2
                };
                layout.insertion_base = (insertion_base.x, insertion_base.y, insertion_base.z);
                layout.min_extents = (min_extents.x, min_extents.y, min_extents.z);
                layout.max_extents = (max_extents.x, max_extents.y, max_extents.z);
                layout.min_limits = (min_limits.x, min_limits.y);
                layout.max_limits = (max_limits.x, max_limits.y);
                break;
            }
        }
    }

    /// Mirror the active space (Model tile-mode vs a paper layout) into the
    /// document's persisted settings so it round-trips on save: the `$TILEMODE`
    /// header (`show_model_space`) and the `CTAB` current-tab variable. The
    /// reader restores it via [`current_layout`] on open. Called whenever the
    /// active layout changes; the file otherwise always reopened in Model.
    pub fn sync_active_space_to_document(&mut self) {
        self.document.header.show_model_space = self.current_layout == "Model";
        crate::scene::drawing_vars::set_saved_active_layout(&mut self.document, &self.current_layout);
    }

    /// Record the entered viewport (or the sheet, in paper space) as the
    /// current layout's last active viewport, which reopens it (#1670).
    pub fn sync_active_viewport_to_document(&mut self) {
        let Some(handle) = self.current_layout_object_handle() else {
            return;
        };
        let sheet = match self.document.objects.get(&handle) {
            Some(ObjectType::Layout(layout)) => {
                Self::layout_sheet_viewport_handle(&self.document, layout)
            }
            _ => return,
        };
        let active = self.active_viewport.unwrap_or(sheet);
        if let Some(ObjectType::Layout(layout)) = self.document.objects.get_mut(&handle) {
            layout.viewport = active;
        }
    }

    /// The model viewport the current layout was saved with entered: its
    /// last active viewport when that is an on viewport other than the sheet.
    pub fn saved_entered_viewport(&self) -> Option<Handle> {
        let handle = self.current_layout_object_handle()?;
        let Some(ObjectType::Layout(layout)) = self.document.objects.get(&handle) else {
            return None;
        };
        let viewport = layout.viewport;
        let sheet = Self::layout_sheet_viewport_handle(&self.document, layout);
        if viewport.is_null() || viewport == sheet {
            return None;
        }
        match self.document.get_entity(viewport) {
            Some(EntityType::Viewport(vp))
                if vp.common.owner_handle == layout.block_record && !vp.status.is_off =>
            {
                Some(viewport)
            }
            _ => None,
        }
    }

}

impl Scene {
    /// Hatch / 2-D-solid fills for a content viewport, with its frozen layers
    /// removed. Cached per frozen-set signature (viewports sharing a frozen set
    /// share the build). No frozen layers → the shared unfiltered set.
    pub(crate) fn hatch_models_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
        tint_selected: bool,
    ) -> Arc<Vec<HatchModel>> {
        let tint_selected = tint_selected && self.current_layout != "Model";
        if viewport.is_null() && frozen.is_empty() {
            return self.hatch_models_arc_for_view(tint_selected);
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, self.current_layout.clone(), sig);
        let sel = if tint_selected {
            self.selected_hatch_sig()
        } else {
            0
        };
        if let Some((e, s, arc)) = self.caches.frozen_hatch_cache.borrow().get(&key) {
            if *e == self.geometry_epoch && *s == sel {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.synced_hatch_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
            tint_selected,
        ));
        self.caches.frozen_hatch_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, sel, Arc::clone(&arc)));
        arc
    }

    /// Wipeout fills for a content viewport, with its frozen layers removed.
    pub(crate) fn wipeout_models_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<HatchModel>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.wipeout_models_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig =
            scale.map_or(0, |handle| handle.value()) ^ u64::from(all_visible).rotate_left(61);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, self.current_layout.clone(), sig);
        {
            let reuse = match self.caches.frozen_wipeout_cache.borrow().get(&key) {
                Some((cached_epoch, arc))
                    if self.wipeout_cache_current(*cached_epoch, arc.is_empty()) =>
                {
                    Some(Arc::clone(arc))
                }
                _ => None,
            };
            if let Some(arc) = reuse {
                // Carry the entry forward, as the model-space path does: left
                // at its original epoch it would eventually fall below
                // `geometry_journal_floor`, where no delta can be replayed and
                // every later edit rebuilds it again.
                if let Some((e, _)) = self.caches.frozen_wipeout_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let arc = Arc::new(self.wipeout_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            (!viewport.is_null()).then_some(viewport),
        ));
        self.caches.frozen_wipeout_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Point clouds of the model content, with a content viewport's frozen
    /// layers removed. The paper sheet shows none. The set is rebuilt per
    /// geometry epoch; unchanged clouds keep their placed points (and GPU
    /// buffers) by key.
    pub(crate) fn point_clouds_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
        paper_sheet: bool,
    ) -> Arc<crate::scene::model::point_cloud::PointCloudSet> {
        use crate::scene::model::point_cloud;
        static EMPTY: std::sync::OnceLock<Arc<point_cloud::PointCloudSet>> =
            std::sync::OnceLock::new();
        let empty = || Arc::clone(EMPTY.get_or_init(Default::default));
        if paper_sheet {
            return empty();
        }
        let target_block = self.content_render_block_handle();
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        // Viewports sharing a frozen set only share the build when the walk
        // matches too: the active (walked) viewport must not leak its set.
        let key = (
            target_block,
            Self::frozen_layers_sig(frozen) ^ Self::layer_walk_sig(layer_walk),
        );
        if let Some((epoch, arc)) = self.caches.point_cloud_cache.borrow().get(&key) {
            if *epoch == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = if point_cloud::any_definition(&self.document) {
            let depths = rustc_hash::FxHashMap::default();
            let visibility_reveal = self.visibility_reveal_handles();
            let graph = render_graph::RenderSceneGraph::new(
                &self.document,
                (!frozen.is_empty()).then_some(frozen),
                None,
                true,
                &depths,
            )
            .with_visibility_reveal(&visibility_reveal)
            .with_layer_walk(layer_walk);
            let mut clouds = Vec::new();
            graph.walk_root(
                self.render_scene_root(target_block),
                |entity, context| {
                    context.is_instanced()
                        || !self.entity_temporarily_hidden(entity.common().handle)
                },
                |entity, context| {
                    let EntityType::Extended(extended) = entity else {
                        return;
                    };
                    let document::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data
                    else {
                        return;
                    };
                    let transform = context.is_instanced().then_some(&context.transform);
                    // Object Color stylization draws the cloud in its own colour.
                    let common = entity.common();
                    let color = if matches!(common.color, document::types::Color::ByLayer) {
                        self.document
                            .layers
                            .get(&common.layer)
                            .map(|layer| layer.color)
                            .unwrap_or(common.color)
                    } else {
                        common.color
                    };
                    let object_color = color.rgb().map_or([255; 3], |(r, g, b)| [r, g, b]);
                    clouds.extend(point_cloud::placed(
                        &self.document,
                        data,
                        object_color,
                        transform,
                        &point_cloud::hidden(data),
                    ));
                },
            );
            if clouds.is_empty() {
                empty()
            } else {
                Arc::new(point_cloud::PointCloudSet {
                    point_size: point_cloud::point_size(&self.document),
                    clouds,
                })
            }
        } else {
            empty()
        };
        self.caches.point_cloud_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Wireframe 2D draws no cloud points: each shown cloud whose file is
    /// found draws its extents box and a message instead, unless
    /// POINTCLOUD2DVSDISPLAY is 1.
    // ponytail: model-space clouds only, rebuilt per frame; cache by epoch if
    // drawings with many clouds make it show.
    pub(crate) fn point_cloud_2d_wires(&self) -> Vec<WireModel> {
        use crate::scene::model::point_cloud::{resolve_source, setting, setting_value};
        if !crate::scene::model::point_cloud::any_definition(&self.document)
            || setting("POINTCLOUD2DVSDISPLAY")
                .is_some_and(|s| setting_value(&self.document, s) == 1)
        {
            return Vec::new();
        }
        let mut wires = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Extended(extended) = entity else {
                continue;
            };
            let document::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data else {
                continue;
            };
            let common = entity.common();
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(common.handle)
                || self
                    .document
                    .layers
                    .get(&common.layer)
                    .is_some_and(|layer| layer.is_off() || layer.is_frozen())
                || self.layer_walk_hidden(self.document.layers.get(&common.layer))
                || resolve_source(&self.document, data).is_none()
            {
                continue;
            }
            let color = if matches!(common.color, document::types::Color::ByLayer) {
                self.document
                    .layers
                    .get(&common.layer)
                    .map(|layer| layer.color)
                    .unwrap_or(common.color)
            } else {
                common.color
            };
            let (r, g, b) = color.rgb().unwrap_or((255, 255, 255));
            wires.push(WireModel::solid_f64(
                common.handle.value().to_string(),
                crate::entities::extended::point_cloud_2d_style_lines(data),
                [
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                ],
                false,
            ));
        }
        wires
    }

    /// Image / OLE models for a content viewport, with its frozen layers removed.
    pub(crate) fn images_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<ImageModel>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.images_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        // The walked (interaction) viewport must not share its build with a
        // frozen-set twin that renders un-walked.
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, sig);
        if let Some((e, arc)) = self.caches.frozen_image_cache.borrow().get(&key) {
            if *e == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.image_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
        ));
        self.caches.frozen_image_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Solid meshes for a content viewport, with its frozen layers removed.
    pub(crate) fn meshes_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<MeshLodSet>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.meshes_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, self.current_layout.clone(), sig);
        if let Some((e, arc)) = self.caches.frozen_mesh_cache.borrow().get(&key) {
            if *e == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.mesh_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
        ));
        self.caches.frozen_mesh_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// True when `handle`'s entity is a Civil / AEC object
    /// (`ExtendedEntityData::Arch`). The codec cannot rewrite its class data
    /// yet, so a move would shift only its cached picture and leave the
    /// object where it was (#3394); transform paths skip it.
    pub fn is_fixed_in_place(&self, handle: Handle) -> bool {
        matches!(
            self.document.get_entity(handle),
            Some(EntityType::Extended(x))
                if matches!(x.data, document::entities::ExtendedEntityData::Arch(_))
        )
    }

    /// True when `handle`'s entity sits on a locked layer, or is a point cloud
    /// locked on its own. Locked objects stay visible and snappable, and
    /// selectable unless PICKLOCKED is off; mutation paths must skip them.
    pub fn is_layer_locked(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        is_locked_point_cloud(entity)
            || self
                .document
                .layers
                .get(&entity.common().layer)
                .is_some_and(|l| l.is_locked())
            // REFEDIT with "Lock objects not in working set": only the
            // working set can be edited.
            || (self.refedit_lock
                && self
                    .refedit_keep
                    .as_ref()
                    .is_some_and(|keep| !keep.contains(&handle)))
    }

    /// True when `handle` addresses a reference-only entity (API v9):
    /// plugin-owned display geometry the built-in edit paths treat like a
    /// locked-layer entity while its owning plugin keeps write access through
    /// the plugin host API. Absent entities are never reference-only.
    pub fn is_reference_only(&self, handle: Handle) -> bool {
        self.reference_only.is_marked(handle) && self.document.get_entity(handle).is_some()
    }

    /// API v18 selectable-custom owner-stamp flag. Unlike reference-only,
    /// this never affects hit-testing or ordinary selection.
    pub fn native_edit_disabled(&self, handle: Handle) -> bool {
        self.document
            .get_entity(handle)
            .is_some_and(crate::entities::owner_stamp::native_edit_disabled)
    }

    /// Insert or remove `handle` in the scene-scoped designation set,
    /// returning whether membership changed. Pure storage: no epoch bump, no
    /// undo bookkeeping — those belong to the callers (the plugin host API
    /// mutation and the history channels).
    pub(crate) fn set_reference_only_membership(&mut self, handle: Handle, on: bool) -> bool {
        self.reference_only.set(handle, on)
    }

    /// The name of the locked layer `handle` sits on, if any (for messages).
    pub fn locked_layer_name(&self, handle: Handle) -> Option<String> {
        let name = self
            .document
            .get_entity(handle)
            .map(|e| e.common().layer.clone())?;
        let locked = self
            .document
            .layers
            .get(&name)
            .map(|l| l.is_locked())
            .unwrap_or(false);
        locked.then_some(name)
    }

    /// File-backed visibility shared by top-level and block-definition meshes.
    /// Object isolation is intentionally absent: a retained INSERT must retain
    /// every visible child of its block definition.
    pub(in crate::scene) fn mesh_definition_entity_visible(&self, handle: Handle) -> bool {
        let Some(c) = self.document.get_entity(handle).map(|e| e.common()) else {
            return false;
        };
        if self.display_entity_invisible(c) {
            return false;
        }
        !self.layer_hidden(&c.layer)
    }

    /// Visibility test for a top-level solid mesh entity, mirroring the direct
    /// 2D wire path.
    pub(in crate::scene) fn mesh_entity_visible(&self, handle: Handle) -> bool {
        if !self.mesh_definition_entity_visible(handle) {
            return false;
        }
        if self.entity_temporarily_hidden(handle) {
            return false;
        }
        true
    }

    /// One transformed mesh per block-definition solid instance reached from an
    /// INSERT owned by `layout_block`. Nested INSERTs accumulate their
    /// transform. Empty when no block solids exist. (#123)
    pub(in crate::scene) fn instanced_block_meshes(
        &self,
        layout_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        live_section: Option<&LiveSection>,
        // `Some`: only the placements of these INSERTs, as when patching a
        // cached list after they changed.
        targets: Option<&HashSet<Handle>>,
    ) -> Vec<MeshLodSet> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let depth_map = self.draw_depth_map();
        let visibility_reveal = self.visibility_reveal_handles();
        let layer_walk = self.render_space_layer_walk(layout_block, viewport);
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut out = Vec::new();
        // Skip block subtrees that hold no mesh at all, the way the instanced
        // hatch walk skips hatch-free ones. Without it every INSERT's whole
        // definition tree is enumerated to ask, which on a drawing whose
        // blocks carry no solids is ~795 ms spent producing nothing. The
        // presence test is memoised across instances by block name.
        let mut mesh_block_memo = std::collections::HashMap::new();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                if context.is_instanced() {
                    return true;
                }
                let handle = entity.common().handle;
                if self.entity_temporarily_hidden(handle) {
                    return false;
                }
                if !targets.is_none_or(|targets| {
                    matches!(entity, EntityType::Insert(_)) && targets.contains(&handle)
                }) {
                    return false;
                }
                // The graph walks at `self.annotation_scale`, so the presence
                // test has to resolve representations at that same scale: at
                // a different one an annotative INSERT can resolve to another
                // block, and skipping on that answer would hide real meshes.
                let block_uses: Vec<_> = render_graph::entity_render_block_uses(
                    &self.document,
                    entity,
                    self.annotation_scale,
                )
                .into_iter()
                .filter(|block_use| block_use.active)
                .collect();
                block_uses.is_empty()
                    || block_uses.into_iter().any(|block_use| {
                        render_graph::block_contains_mesh(
                            &self.document,
                            &block_use.insert.block_name,
                            &mut mesh_block_memo,
                        )
                    })
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let handle = entity.common().handle;
                let Some(original) = self.block_meshes.get(&handle) else {
                    return;
                };
                let sectioned;
                let set = if let Some((section, body)) = live_section.and_then(|section| {
                    self.section_source_body(handle).map(|body| (section, body))
                }) {
                    match Self::section_body(
                        &body,
                        &section.data,
                        section.slice_depth,
                        Some(&context.transform),
                    ) {
                        Ok(Some(body)) => {
                            if let Some((mut mesh, _, _)) =
                                self.prepare_solid_model_display(handle, &body)
                            {
                                mesh.prepare_instance_source(handle);
                                sectioned = mesh;
                                &sectioned
                            } else {
                                original
                            }
                        }
                        Ok(None) => return,
                        Err(()) => original,
                    }
                } else {
                    original
                };
                let inherit = self.mesh_inherit_for_path(&context.insert_path);
                let own_alpha = set.display_color().map_or(1.0, |color| color[3]);
                let mut transformed = transform_block_mesh_lod_set(set, &context.transform);
                if let Some(color) =
                    self.block_mesh_override_color(entity, handle, inherit.as_ref(), own_alpha)
                {
                    transformed.instance_color = Some(color);
                    if let Some(material) = transformed.material.as_mut() {
                        if material.handle.is_none() {
                            material.diffuse = color;
                        }
                    }
                }
                if let Some(mut material) =
                    self.block_mesh_override_material(entity, inherit.as_ref())
                {
                    // A material that takes the object's colour takes this
                    // child's, not the insert's it was resolved with.
                    if material.diffuse_by_object {
                        if let Some(own) = transformed.instance_color.or(set.display_color()) {
                            material.diffuse = [own[0], own[1], own[2], material.diffuse[3]];
                            material.ambient = [own[0], own[1], own[2]];
                        }
                    }
                    material.apply_to_with_face_overrides(
                        &mut transformed,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                }
                if self.count_display.is_some() || self.visibility_revealed(context.root_handle) {
                    let own = transformed
                        .instance_color
                        .or(set.display_color())
                        .unwrap_or([1.0; 4]);
                    let color = self.count_color(context.root_handle, own, self.current_bg());
                    transformed.instance_color = Some(color);
                    // A material pinned the edges to the object's colour.
                    if transformed.edge_color.is_some() {
                        transformed.edge_color = Some(color);
                    }
                    if let Some(material) = transformed.material.as_mut() {
                        material.diffuse = color;
                    }
                }
                transformed.instance_handle = Some(context.root_handle);
                transformed.effective_layer = Some(std::sync::Arc::from(
                    context.effective_layer(&entity.common().layer),
                ));
                out.push(transformed);
            },
        );
        out
    }

    pub(in crate::scene) fn mesh_inherit_for_path(&self, path: &[DxfInsert]) -> Option<BlockMeshInherit> {
        let first = path.first()?;
        let entity = EntityType::Insert(first.clone());
        let bg = self.current_bg();
        let insert_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::render_style_for(&self.document, &entity).0,
            bg,
        );
        let layer0_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::layer_render_style(&self.document, &first.common.layer)
                .color,
            bg,
        );
        let mut inherit = BlockMeshInherit {
            insert_color,
            layer0_color,
            insert_material: crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                &entity,
                insert_color,
                None,
                self.material_base_dir.as_deref(),
            ),
            layer0_material: crate::scene::model::material_model::resolve_layer_material_with_base(
                &self.document,
                &first.common.layer,
                layer0_color,
                self.material_base_dir.as_deref(),
            ),
        };
        for insert in &path[1..] {
            inherit = self.chain_mesh_inherit(insert, &inherit);
        }
        Some(inherit)
    }

    /// Background colour for the current layout (model vs paper).
    pub(crate) fn current_bg(&self) -> [f32; 4] {
        if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        }
    }

    /// Chain block-child colour inheritance into a nested INSERT, mirroring
    /// `expand_insert`: ByBlock keeps the parent source; a nested insert on
    /// layer "0" with ByLayer colour adopts the parent layer-0 target; else it
    /// uses its own resolved colour / layer. Returned colours are bg-adapted.
    pub(in crate::scene) fn chain_mesh_inherit(
        &self,
        ins: &document::entities::Insert,
        parent: &BlockMeshInherit,
    ) -> BlockMeshInherit {
        use document::types::Color;
        let bg = self.current_bg();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&ins.common.layer);
        let insert_entity = EntityType::Insert(ins.clone());
        let has_book_color =
            crate::scene::view::render::has_resolved_book_color(&self.document, &insert_entity);
        let child_ins_color = if !has_book_color && ins.common.color == Color::ByBlock {
            parent.insert_color
        } else if !has_book_color && on_l0 && ins.common.color == Color::ByLayer {
            parent.layer0_color
        } else {
            crate::scene::view::render::adapt_to_bg(
                crate::scene::view::render::render_style_for(
                    &self.document,
                    &EntityType::Insert(ins.clone()),
                )
                .0,
                bg,
            )
        };
        let child_l0 = if on_l0 {
            parent.layer0_color
        } else {
            crate::scene::view::render::adapt_to_bg(
                crate::scene::view::render::layer_render_style(&self.document, &ins.common.layer)
                    .color,
                bg,
            )
        };
        let child_insert_material = if ins.common.material_flags == 1 {
            parent.insert_material.clone()
        } else if on_l0 && ins.common.material_flags == 0 {
            parent.layer0_material.clone()
        } else {
            crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                &insert_entity,
                child_ins_color,
                Some(&parent.insert_material),
                self.material_base_dir.as_deref(),
            )
        };
        let child_l0_material = if on_l0 {
            parent.layer0_material.clone()
        } else {
            crate::scene::model::material_model::resolve_layer_material_with_base(
                &self.document,
                &ins.common.layer,
                child_l0,
                self.material_base_dir.as_deref(),
            )
        };
        BlockMeshInherit {
            insert_color: child_ins_color,
            layer0_color: child_l0,
            insert_material: child_insert_material,
            layer0_material: child_l0_material,
        }
    }

    /// Per-instance colour override for a block-internal solid mesh: ByBlock →
    /// insert colour, layer-0 + ByLayer → insert-layer colour, else `None`
    /// (keep the cached own-layer colour). Applies the same REFEDIT fade as
    /// `recolor_meshes`, keyed on the inner solid handle. (#221)
    pub(in crate::scene) fn block_mesh_override_color(
        &self,
        e: &EntityType,
        h: Handle,
        inherit: Option<&BlockMeshInherit>,
        own_alpha: f32,
    ) -> Option<[f32; 4]> {
        let inherit = inherit?;
        use document::types::Color;
        let common = e.common();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&common.layer);
        let has_book_color = crate::scene::view::render::has_resolved_book_color(&self.document, e);
        let mut c = if !has_book_color && common.color == Color::ByBlock {
            inherit.insert_color
        } else if !has_book_color && on_l0 && common.color == Color::ByLayer {
            // Inherit the insert layer's RGB but keep the solid's own alpha,
            // matching the wire/hatch path (render_style_for_block_sub).
            [
                inherit.layer0_color[0],
                inherit.layer0_color[1],
                inherit.layer0_color[2],
                own_alpha,
            ]
        } else {
            return None;
        };
        if let Some(keep) = &self.refedit_keep {
            if !keep.contains(&h) {
                c = crate::scene::cache::block_cache::refedit_fade(c, self.current_bg());
            }
        }
        Some(c)
    }

    pub(in crate::scene) fn block_mesh_override_material(
        &self,
        entity: &EntityType,
        inherit: Option<&BlockMeshInherit>,
    ) -> Option<crate::scene::model::material_model::MeshMaterial> {
        let inherit = inherit?;
        let common = entity.common();
        if common.material_flags == 1 {
            Some(inherit.insert_material.clone())
        } else if common.material_flags == 0
            && crate::scene::view::render::is_effective_layer_zero(&common.layer)
        {
            Some(inherit.layer0_material.clone())
        } else {
            None
        }
    }

}
