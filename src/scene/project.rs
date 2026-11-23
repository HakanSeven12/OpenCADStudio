// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    /// Collect model-space wires projected into paper space for all (or one specific)
    /// user viewports.  `only_vp = Some(h)` restricts output to that viewport.
    /// `plot` adds the edges of 3-D meshes, which the screen draws on the GPU
    /// and the plot must draw as wires; such a set is not cached. `shade` is
    /// the plotted render mode of those meshes (see [`Self::mesh_plot_wires`]).
    pub(super) fn viewport_content_wires(
        &self,
        paper_block: Handle,
        only_vp: Option<Handle>,
        exclude_vp: Option<Handle>,
        plot: bool,
        shade: Option<document::entities::ViewportRenderMode>,
    ) -> Vec<WireModel> {
        use document::entities::Viewport;

        let (_, _, viewport_handles) = self.paper_viewport_handles();
        let viewports: Vec<&Viewport> = viewport_handles
            .iter()
            .filter_map(|handle| match self.document.get_entity(*handle) {
                Some(EntityType::Viewport(vp)) => Some(&**vp),
                _ => None,
            })
            .filter(|vp| {
                vp.common.owner_handle == paper_block
                    && vp.is_on()
                    && only_vp.is_none_or(|h| vp.common.handle == h)
                    && (exclude_vp != Some(vp.common.handle))
            })
            .collect();

        if viewports.is_empty() {
            return vec![];
        }

        let mut result = Vec::new();

        for vp in viewports {
            let vp_handle = vp.common.handle;

            // ── Fast path: return cached projected wires ──────────────────
            if !plot {
                let cache = self.paper_projected_cache.borrow();
                if let Some((cached_epoch, ref wires)) = cache.get(&vp_handle) {
                    if *cached_epoch == self.geometry_epoch {
                        result.extend_from_slice(wires);
                        continue;
                    }
                }
            }

            // ── Cache miss: compute projection ────────────────────────────

            // Use camera_for_viewport so the axes match the GPU renderer exactly.
            let cam_frame = match self.camera_for_viewport(vp_handle) {
                Some(c) => c,
                None => continue,
            };
            let view_right = cam_frame.rotation * glam::Vec3::X;
            let view_up = cam_frame.rotation * glam::Vec3::Y;

            // Scale (paper units per model unit) comes straight from the camera
            // the GPU uses: the model height shown is `2 * ortho_size`, mapped
            // onto `vp.height` of paper. `camera_for_viewport` already made the
            // saved-view-vs-auto-fit decision (with the twist-correct overlap
            // test), so deriving scale from it keeps the CPU projection (used
            // for hit-test / snap / fit) locked to the GPU render — no second,
            // independently-computed scale that could disagree under a twist.
            let view_height_eff = (cam_frame.ortho_size() * 2.0) as f64;
            let scale = if view_height_eff > 1e-9 {
                (vp.height / view_height_eff) as f32
            } else {
                1.0
            };

            let pcx = vp.center.x as f32;
            let pcy = vp.center.y as f32;
            let pcz = vp.center.z as f32;
            let hw = (vp.width / 2.0) as f32;
            let hh = (vp.height / 2.0) as f32;

            // ── Use cached tessellation (model_wires_for_viewport_arc) ────
            // This eliminates the per-frame tessellate_one() loop that was here
            // previously; tessellation is now O(1) on navigation frames.
            // Pass 0.0 for screen height — the CPU-projection / hit-test
            // path wants the full-fidelity (no-LOD-stub) wire list,
            // regardless of paper zoom.
            // A plot has no later frame to catch up on a zoom that just ended.
            if plot {
                self.viewport_complex_lt_factor(vp_handle, true);
            }
            let model_wires = self.model_wires_for_viewport_arc(vp_handle, 0.0);
            let mut model_wires = crate::scene::model::instance_model::expand_stubs(&model_wires);
            if plot {
                let frozen: HashSet<Handle> = vp.frozen_layers.iter().copied().collect();
                let meshes = self.meshes_for_viewport(vp_handle, &frozen);
                let view = (cam_frame.rotation * glam::Vec3::Z).as_dvec3().to_array();
                let mesh_wires = self.mesh_plot_wires(&meshes, view, Some(vp_handle), shade);
                model_wires.to_mut().extend(mesh_wires);
                for wire in model_wires.to_mut() {
                    wire.resample_plot_curve();
                }
            }

            // ── Project and clip wires into viewport ──────────────────────
            let vp_x0 = pcx - hw;
            let vp_x1 = pcx + hw;
            let vp_y0 = pcy - hh;
            let vp_y1 = pcy + hh;

            let plot_clip = if plot && crate::entities::viewport::is_clipped(vp) {
                let ring = self.clip_boundary_polygon(vp.clip_boundary_handle, pcz);
                (ring.len() >= 3).then(|| {
                    ring.into_iter()
                        .map(|[x, y, _]| [x, y])
                        .collect::<std::sync::Arc<[[f32; 2]]>>()
                })
            } else {
                None
            };

            // camera_dist: how far the camera is from the target plane.
            let use_perspective = vp.status.perspective && vp.lens_length > 1.0;
            let camera_dist = if use_perspective {
                (vp.view_height as f32 * vp.lens_length as f32 / 24.0).max(0.001)
            } else {
                0.0
            };

            let mut projected: Vec<WireModel> = Vec::new();

            // Precompute precision-stable WCS-space projection inputs in
            // f64. The previous f32 inner loop suffered catastrophic
            // cancellation on UTM-scale drawings: `(wire_offset_rel -
            // target_offset_rel).dot(view_right) - view_center` is a
            // small paper offset computed by subtracting two values at
            // ~5e6 magnitude — f32 ULP there is ~0.5 m, so paper output
            // jittered by cm even when the actual model was clean.
            //
            // Do everything WCS-relative in f64; cast to f32 only at the
            // final paper position.
            // Display centre = the camera's target, in WCS. `camera_for_viewport`
            // already folded view_center through the (twisted) view basis and
            // applied the empty-WCS auto-fit, so taking its target keeps the CPU
            // projection identical to the GPU renderer under any twist.
            let display_center_x = cam_frame.target.x + [0.0_f64; 3][0];
            let display_center_y = cam_frame.target.y + [0.0_f64; 3][1];
            let display_center_z = cam_frame.target.z + [0.0_f64; 3][2];
            let view_right_d = (
                view_right.x as f64,
                view_right.y as f64,
                view_right.z as f64,
            );
            let view_up_d = (view_up.x as f64, view_up.y as f64, view_up.z as f64);
            let view_fwd = cam_frame.rotation * glam::Vec3::Z;
            let view_fwd_d = (view_fwd.x as f64, view_fwd.y as f64, view_fwd.z as f64);
            let camera_dist_d = camera_dist as f64;
            let scale_d = scale as f64;
            let pcx_d = pcx as f64;
            let pcy_d = pcy as f64;
            // Project one ABSOLUTE-WCS model point (f64) onto the paper sheet.
            // Shared by the polyline points, snap points and key vertices so the
            // hit-test / snap geometry lands in the same paper frame the wire is
            // drawn in — otherwise snaps and the click-AABB stay in model (UTM)
            // space and the cursor never reaches them.
            let proj_abs = |ax: f64, ay: f64, az: f64| -> [f32; 3] {
                let mp_x = ax - display_center_x;
                let mp_y = ay - display_center_y;
                let mp_z = az - display_center_z;
                let u = mp_x * view_right_d.0 + mp_y * view_right_d.1 + mp_z * view_right_d.2;
                let v = mp_x * view_up_d.0 + mp_y * view_up_d.1 + mp_z * view_up_d.2;
                if use_perspective {
                    let d_vd = mp_x * view_fwd_d.0 + mp_y * view_fwd_d.1 + mp_z * view_fwd_d.2;
                    let fwd = camera_dist_d - d_vd;
                    if fwd <= 0.001 {
                        return [f32::NAN; 3];
                    }
                    let factor = camera_dist_d / fwd;
                    [
                        (pcx_d + u * factor * scale_d) as f32,
                        (pcy_d + v * factor * scale_d) as f32,
                        pcz,
                    ]
                } else {
                    [
                        (pcx_d + u * scale_d) as f32,
                        (pcy_d + v * scale_d) as f32,
                        pcz,
                    ]
                }
            };
            let in_vp = |x: f32, y: f32| x >= vp_x0 && x <= vp_x1 && y >= vp_y0 && y <= vp_y1;
            // DVIEW Clip: depth toward the eye from the target, kept between
            // the back and front planes the viewport switches on — the cut the
            // screen makes with its near/far planes.
            let depth_of = |p: glam::DVec3| {
                (p.x - display_center_x) * view_fwd_d.0
                    + (p.y - display_center_y) * view_fwd_d.1
                    + (p.z - display_center_z) * view_fwd_d.2
            };
            let front = vp.status.front_clipping && vp.status.front_clip_not_at_eye;
            let depth_range = (front || vp.status.back_clipping).then(|| {
                (
                    if vp.status.back_clipping {
                        vp.back_clip_z
                    } else {
                        f64::NEG_INFINITY
                    },
                    if front { vp.front_clip_z } else { f64::INFINITY },
                )
            });

            for wire in model_wires.iter() {
                let marker_view_height = wire.point_marker.map_or(view_height_eff, |marker| {
                    if !use_perspective {
                        return view_height_eff;
                    }
                    let relative = marker.origin
                        - glam::DVec3::new(display_center_x, display_center_y, display_center_z);
                    let depth = relative.x * view_fwd_d.0
                        + relative.y * view_fwd_d.1
                        + relative.z * view_fwd_d.2;
                    view_height_eff * (camera_dist_d - depth).max(0.001) / camera_dist_d.max(0.001)
                });
                let depth_pts: Vec<([f32; 3], f64)> = wire
                    .points
                    .iter()
                    .enumerate()
                    .map(|(pi, &[mx, my, mz])| {
                        if mx.is_nan() || my.is_nan() || mz.is_nan() {
                            return ([f32::NAN; 3], 0.0);
                        }
                        let point = wire.point_world(pi, marker_view_height);
                        (proj_abs(point.x, point.y, point.z), depth_of(point))
                    })
                    .collect();
                let depth_clipped = depth_range.is_some_and(|(back, front)| {
                    depth_pts
                        .iter()
                        .any(|(p, d)| p[0].is_finite() && (*d < back || *d > front))
                });
                let projected_pts: Vec<[f32; 3]> = match depth_range {
                    Some((back, front)) if depth_clipped => {
                        clip_polyline_to_depth(&depth_pts, back, front)
                    }
                    _ => depth_pts.iter().map(|(p, _)| *p).collect(),
                };

                // SDF glyph quads ride `text_verts` in absolute WCS, so they need
                // the same projection the points get — a bare clone leaves the
                // text at model (UTM) coordinates while its own dimension lines
                // move to the sheet, putting the glyphs kilometres off the page
                // (issue #385 for layout plots). Cull per glyph on the quad's
                // centroid: the text analogue of `clip_polyline_to_rect`, since a
                // glyph can't be split at the viewport border.
                let mut projected_text = if wire.text_verts.is_empty() {
                    Vec::new()
                } else {
                    model::wire_model::map_text_verts(&wire.text_verts, |x, y, z| {
                        let p = proj_abs(x, y, z);
                        (p[0] as f64, p[1] as f64, p[2] as f64)
                    })
                    .chunks_exact(6)
                    .filter(|quad| {
                        let (sx, sy) = quad.iter().fold((0.0f32, 0.0f32), |(ax, ay), v| {
                            (ax + v.pos[0] + v.pos_low[0], ay + v.pos[1] + v.pos_low[1])
                        });
                        let (cx, cy) = (sx / 6.0, sy / 6.0);
                        cx.is_finite() && cy.is_finite() && in_vp(cx, cy)
                    })
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>()
                };

                // Fills (2-D solids, wide and thickened polyline bands, 3-D
                // faces) project triangle by triangle and are clipped to the
                // viewport; the clipped convex polygon is re-fanned.
                let mut projected_fill: Vec<[f32; 3]> = Vec::new();
                for (ti, tri) in wire.fill_tris.chunks_exact(3).enumerate() {
                    let corner = |k: usize| {
                        let low = wire.fill_tris_low.get(ti * 3 + k).copied();
                        let low = low.unwrap_or([0.0; 3]);
                        let p = proj_abs(
                            tri[k][0] as f64 + low[0] as f64,
                            tri[k][1] as f64 + low[1] as f64,
                            tri[k][2] as f64 + low[2] as f64,
                        );
                        [p[0], p[1]]
                    };
                    if let Some((back, front)) = depth_range {
                        let outside = (0..3).any(|k| {
                            let low = wire.fill_tris_low.get(ti * 3 + k).copied();
                            let low = low.unwrap_or([0.0; 3]);
                            let d = depth_of(glam::DVec3::new(
                                tri[k][0] as f64 + low[0] as f64,
                                tri[k][1] as f64 + low[1] as f64,
                                tri[k][2] as f64 + low[2] as f64,
                            ));
                            d < back || d > front
                        });
                        // ponytail: a fill crossing a clip plane drops whole; split it if plots need the sliver.
                        if outside {
                            continue;
                        }
                    }
                    let corners = [corner(0), corner(1), corner(2)];
                    if corners
                        .iter()
                        .any(|p| !p[0].is_finite() || !p[1].is_finite())
                    {
                        continue;
                    }
                    let clipped = clip_polygon_to_rect(&corners, vp_x0, vp_y0, vp_x1, vp_y1);
                    for k in 1..clipped.len().saturating_sub(1) {
                        for p in [clipped[0], clipped[k], clipped[k + 1]] {
                            projected_fill.push([p[0], p[1], pcz]);
                        }
                    }
                }

                // Fast AABB pre-reject.
                let any_near = projected_pts.iter().any(|&[x, y, _]| {
                    x.is_finite()
                        && y.is_finite()
                        && x >= vp_x0 - 1.0
                        && x <= vp_x1 + 1.0
                        && y >= vp_y0 - 1.0
                        && y <= vp_y1 + 1.0
                });
                let (min_x, max_x, min_y, max_y) =
                    projected_pts.iter().filter(|p| p[0].is_finite()).fold(
                        (
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                        ),
                        |(mnx, mxx, mny, mxy), &[x, y, _]| {
                            (mnx.min(x), mxx.max(x), mny.min(y), mxy.max(y))
                        },
                    );
                let aabb_hits =
                    max_x >= vp_x0 && min_x <= vp_x1 && max_y >= vp_y0 && min_y <= vp_y1;
                // A text-only wire (TEXT / MTEXT carry no stroke `points` since
                // text went SDF-only) has nothing to pre-reject on, so gate both
                // rejections on the glyphs too or it never reaches the sheet.
                if !any_near && !aabb_hits && projected_text.is_empty() && projected_fill.is_empty()
                {
                    continue;
                }

                let stationed = !depth_clipped && wire.pattern_stations.len() > wire.points.len();
                let source_length = stationed.then(|| wire.pattern_stations[wire.points.len()]);
                let (clipped, mut clipped_stations) = clip_polyline_to_rect(
                    &projected_pts,
                    stationed.then(|| &wire.pattern_stations[..wire.points.len()]),
                    vp_x0,
                    vp_y0,
                    vp_x1,
                    vp_y1,
                    pcz,
                );
                if clipped.is_empty() && projected_text.is_empty() && projected_fill.is_empty() {
                    continue;
                }

                // Paper-space AABB of the clipped polyline — the cloned model
                // (UTM) AABB would make click_hit's screen-projected pre-reject
                // discard the wire (box selection has no pre-reject, which is why
                // it kept working while picking didn't).
                let mut pmnx = f32::INFINITY;
                let mut pmny = f32::INFINITY;
                let mut pmxx = f32::NEG_INFINITY;
                let mut pmxy = f32::NEG_INFINITY;
                for &[x, y, _] in clipped.iter().filter(|p| p[0].is_finite()) {
                    pmnx = pmnx.min(x);
                    pmny = pmny.min(y);
                    pmxx = pmxx.max(x);
                    pmxy = pmxy.max(y);
                }
                // Glyphs and fills count towards the paper AABB as well — a
                // text-only wire would otherwise stay UNBOUNDED and never pick.
                let glyphs = projected_text
                    .iter()
                    .map(|v| (v.pos[0] + v.pos_low[0], v.pos[1] + v.pos_low[1]));
                for (x, y) in glyphs.chain(projected_fill.iter().map(|p| (p[0], p[1]))) {
                    if x.is_finite() && y.is_finite() {
                        pmnx = pmnx.min(x);
                        pmny = pmny.min(y);
                        pmxx = pmxx.max(x);
                        pmxy = pmxy.max(y);
                    }
                }

                // Project snap points + key vertices into the same paper frame,
                // keeping only those inside the viewport rect, so endpoint /
                // midpoint / centre snaps land on the visible sheet geometry
                // instead of the model's UTM coordinates.
                let snap_pts: Vec<(glam::DVec3, model::wire_model::SnapHint)> = wire
                    .snap_pts
                    .iter()
                    .filter_map(|(w, h)| {
                        let p = proj_abs(w.x, w.y, w.z);
                        (p[0].is_finite() && in_vp(p[0], p[1]))
                            .then(|| (glam::DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64), *h))
                    })
                    .collect();
                let key_vertices: Vec<[f64; 3]> = wire
                    .key_vertices
                    .iter()
                    .filter_map(|&[kx, ky, kz]| {
                        let p = proj_abs(kx, ky, kz);
                        (p[0].is_finite() && in_vp(p[0], p[1]))
                            .then(|| [p[0] as f64, p[1] as f64, p[2] as f64])
                    })
                    .collect();

                let adapted = view::render::adapt_to_bg(wire.color, self.paper_bg_color);
                // Glyphs carry their own colour: use the same screen-only
                // dimming as wires, preserving source RGBA when plotting.
                for v in &mut projected_text {
                    let [tr, tg, tb, ta] = view::render::adapt_to_bg(v.color, self.paper_bg_color);
                    v.color = super::viewport_color::projected_color([tr, tg, tb, ta], plot);
                }
                let mut out = wire.clone();
                out.points = clipped;
                out.plot_clip = plot_clip.clone();
                // A wide polyline's band width is a geometric width in model
                // units; the points were just projected into paper units, so
                // the band (and any per-point taper) must follow the same
                // scale or the PDF / print exporter strokes a 15-unit bus bar
                // as 15 mm of paper. Negative values are fixed pixel widths
                // and zero means "use the lineweight" — both stay as they are.
                if out.world_width > 0.0 {
                    out.world_width = wire.world_width * scale;
                    if wire.taper_widths.len() == out.points.len() {
                        out.taper_widths = wire.taper_widths.iter().map(|w| w * scale).collect();
                    } else {
                        // Clipping changed the vertex count, so the per-point
                        // widths no longer line up; fall back to a constant band.
                        out.taper_widths.clear();
                    }
                }
                if out.points.len() != wire.points.len() {
                    out.dash_closure = crate::scene::model::wire_model::DashClosure::Open;
                }
                if let Some(source_length) = source_length {
                    for station in &mut clipped_stations {
                        *station *= scale;
                    }
                    clipped_stations.push(source_length * scale);
                    out.pattern_stations = clipped_stations;
                } else {
                    out.pattern_stations.clear();
                }
                out.text_verts = projected_text;
                out.fill_tris = projected_fill;
                out.fill_tris_low = Vec::new();
                // Searchable runs ride the same projection through the shared
                // helper (origin map + height/advance scale), then cull by
                // run-rect overlap — a run straddling the viewport edge is
                // kept, where origin-only culling would drop visible text.
                out.searchable_text = wire.searchable_text.clone();
                out.map_searchable_runs(
                    &|p| {
                        let q = proj_abs(p[0], p[1], p[2]);
                        [q[0] as f64, q[1] as f64, q[2] as f64]
                    },
                    scale as f64,
                    0.0,
                );
                out.searchable_text.retain(|run| {
                    run.origin.iter().all(|v| v.is_finite())
                        && crate::scene::model::wire_model::run_rect_overlap(
                            [run.origin[0], run.origin[1]],
                            run.rotation,
                            run.adv_width as f64,
                            run.height as f64,
                            [vp_x0 as f64, vp_y0 as f64, vp_x1 as f64, vp_y1 as f64],
                        )
                });
                // Paper coordinates are small sheet units — no relative-to-eye
                // residual is needed, and keeping the model wire's points_low
                // here would add a model-scale offset to the paper points.
                out.points_low = Default::default();
                out.point_marker = None;
                out.snap_pts = snap_pts;
                out.key_vertices = key_vertices;
                // Tangent geometry is in model space and can't be trivially
                // re-expressed in paper coords — drop it (no tangent snap on
                // projected viewport content) rather than snap to UTM.
                out.tangent_geoms = Vec::new();
                // Same for thickness walls: `points` above were projected and
                // clipped into paper coords, and the clone's wall triangles are
                // still model-space. Left in place they would hit-test at a
                // model-scale offset — a stray catch somewhere out on the sheet.
                // Drop them: an extruded entity inside a floating viewport stays
                // selectable by its edges, which do get projected.
                out.pick_tris = Vec::new();
                out.pick_tris_low = Vec::new();
                out.aabb = if pmnx.is_finite() {
                    [pmnx, pmny, pmxx, pmxy]
                } else {
                    WireModel::UNBOUNDED_AABB
                };
                out.color = super::viewport_color::projected_color(adapted, plot);
                out.line_weight_px = wire.line_weight_px;
                // Wire patterns stay in their base model units. Projection
                // normally scales them with the geometry; PSLTSCALE instead
                // keeps dash sizes constant in paper units (the on-screen path
                // applies the same factor through its viewport uniform).
                let dash_scale = if self.document.header.paper_space_linetype_scaling {
                    1.0
                } else {
                    scale
                };
                out.pattern_length = wire.pattern_length * dash_scale;
                out.pattern = wire.pattern.map(|v| v * dash_scale);
                projected.push(out);
            }

            // Store in cache, then extend result.
            if !plot {
                self.paper_projected_cache
                    .borrow_mut()
                    .insert(vp_handle, (self.geometry_epoch, projected.clone()));
            }
            result.extend(projected);
        }

        result
    }

    /// Model-space fills projected into every active paper viewport for vector
    /// plotting. The screen renderer projects these on the GPU; PDF generation
    /// needs the equivalent paper-space geometry explicitly.
    pub fn viewport_plot_fills(
        &self,
    ) -> (
        Vec<(WireModel, f32)>,
        Vec<HatchModel>,
        Vec<HatchModel>,
        Vec<crate::io::pdf_export::PlotImage>,
    ) {
        use document::entities::Viewport;
        use model::hatch_model::HatchPattern;

        if self.current_layout == "Model" {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        }
        let paper_block = self.current_layout_block_handle();
        let model_block = self.model_space_block_handle();
        let (_, _, viewport_handles) = self.paper_viewport_handles();
        let viewports: Vec<&Viewport> = viewport_handles
            .iter()
            .filter_map(|handle| match self.document.get_entity(*handle) {
                Some(EntityType::Viewport(viewport))
                    if viewport.common.owner_handle == paper_block && viewport.is_on() =>
                {
                    Some(&**viewport)
                }
                _ => None,
            })
            .collect();

        let mut pattern_wires = Vec::new();
        let mut projected_hatches = Vec::new();
        let mut projected_wipeouts = Vec::new();
        let mut projected_images = Vec::new();

        for viewport in viewports {
            let Some(camera) = self.camera_for_viewport(viewport.common.handle) else {
                continue;
            };
            let view_right = camera.rotation * glam::Vec3::X;
            let view_up = camera.rotation * glam::Vec3::Y;
            let view_forward = camera.rotation * glam::Vec3::Z;
            let view_height = (camera.ortho_size() * 2.0) as f64;
            let viewport_scale = if view_height > 1e-9 {
                viewport.height / view_height
            } else {
                1.0
            };
            let center_x = viewport.center.x;
            let center_y = viewport.center.y;
            let half_w = viewport.width * 0.5;
            let half_h = viewport.height * 0.5;
            let (xmin, ymin, xmax, ymax) = (
                (center_x - half_w) as f32,
                (center_y - half_h) as f32,
                (center_x + half_w) as f32,
                (center_y + half_h) as f32,
            );
            let perspective = viewport.status.perspective && viewport.lens_length > 1.0;
            let camera_distance = if perspective {
                (viewport.view_height * viewport.lens_length / 24.0).max(0.001)
            } else {
                0.0
            };
            let project_3d = |point: [f64; 3]| -> Option<[f32; 2]> {
                let delta = glam::DVec3::from(point) - camera.target;
                let u = delta.dot(view_right.as_dvec3());
                let v = delta.dot(view_up.as_dvec3());
                let factor = if perspective {
                    let depth = camera_distance - delta.dot(view_forward.as_dvec3());
                    if depth <= 0.001 {
                        return None;
                    }
                    camera_distance / depth
                } else {
                    1.0
                };
                Some([
                    (center_x + u * factor * viewport_scale) as f32,
                    (center_y + v * factor * viewport_scale) as f32,
                ])
            };
            let project = |x: f64, y: f64| project_3d([x, y, 0.0]);

            let frozen: rustc_hash::FxHashSet<Handle> =
                viewport.frozen_layers.iter().copied().collect();
            let mut viewport_clips = vec![vec![
                [xmin as f64, ymin as f64],
                [xmax as f64, ymin as f64],
                [xmax as f64, ymax as f64],
                [xmin as f64, ymax as f64],
            ]];
            if !self.images.is_empty() && crate::entities::viewport::is_clipped(viewport) {
                let boundary = self
                    .clip_boundary_polygon(viewport.clip_boundary_handle, viewport.center.z as f32);
                if boundary.len() >= 3 {
                    viewport_clips.push(
                        boundary
                            .iter()
                            .map(|p| [p[0] as f64, p[1] as f64])
                            .collect(),
                    );
                }
            }
            for mut plot in self.placed_images(
                model_block,
                Some(&frozen),
                self.viewport_scale_handle(viewport.common.handle),
                self.annotation_all_visible(),
                Some(viewport.common.handle),
                true,
            ) {
                let mut valid = true;
                let mut project_point = |high: &mut [f32; 3], low: &mut [f32; 3]| {
                    let point = std::array::from_fn(|i| high[i] as f64 + low[i] as f64);
                    if let Some([x, y]) = project_3d(point) {
                        *high = [x, y, 0.0];
                        *low = [0.0; 3];
                    } else {
                        valid = false;
                    }
                };
                for (high, low) in plot
                    .image
                    .corners
                    .iter_mut()
                    .zip(&mut plot.image.corners_low)
                {
                    project_point(high, low);
                }
                for vertex in &mut plot.image.verts {
                    project_point(&mut vertex.pos, &mut vertex.pos_low);
                }
                for clip in &mut plot.clips {
                    for point in clip {
                        if let Some([x, y]) = project(point[0], point[1]) {
                            *point = [x as f64, y as f64];
                        } else {
                            valid = false;
                        }
                    }
                }
                if !valid {
                    continue;
                }
                if [(0, xmin, xmax), (1, ymin, ymax)]
                    .iter()
                    .any(|&(axis, min, max)| {
                        plot.image.verts.iter().all(|v| v.pos[axis] < min)
                            || plot.image.verts.iter().all(|v| v.pos[axis] > max)
                    })
                {
                    continue;
                }
                plot.clips.extend(viewport_clips.iter().cloned());
                projected_images.push(plot);
            }
            let hatches = self.plot_hatches_for_block(
                model_block,
                Some(&frozen),
                self.viewport_scale_handle(viewport.common.handle),
                self.annotation_all_visible(),
                true,
            );
            for hatch in hatches {
                if matches!(&hatch.pattern, HatchPattern::Pattern(_)) {
                    let mut points = Vec::new();
                    let mut aabb = [
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ];
                    for [a, b] in hatch.pattern_segments_for_plot() {
                        let (Some(a), Some(b)) = (project(a[0], a[1]), project(b[0], b[1])) else {
                            continue;
                        };
                        let Some((ax, ay, bx, by)) =
                            cs_clip(a[0], a[1], b[0], b[1], xmin, ymin, xmax, ymax)
                        else {
                            continue;
                        };
                        if !points.is_empty() {
                            points.push([f32::NAN, f32::NAN, f32::NAN]);
                        }
                        points.push([ax, ay, viewport.center.z as f32]);
                        points.push([bx, by, viewport.center.z as f32]);
                        aabb[0] = aabb[0].min(ax).min(bx);
                        aabb[1] = aabb[1].min(ay).min(by);
                        aabb[2] = aabb[2].max(ax).max(bx);
                        aabb[3] = aabb[3].max(ay).max(by);
                    }
                    if !points.is_empty() {
                        let mut wire = WireModel::solid(
                            hatch
                                .entity
                                .map(|handle| handle.value().to_string())
                                .unwrap_or_else(|| "viewport_hatch_pattern".to_string()),
                            points,
                            hatch.color,
                            false,
                        );
                        wire.effective_layer = hatch.effective_layer.clone();
                        wire.aci = hatch.aci;
                        wire.line_weight_px = hatch.line_weight_px;
                        wire.aabb = aabb;
                        pattern_wires.push((wire, hatch.draw_depth));
                    }
                    continue;
                }
                if let Some(hatch) = project_plot_fill(hatch, &project_3d, xmin, ymin, xmax, ymax) {
                    projected_hatches.push(hatch);
                }
            }

            for wipeout in self.plot_wipeouts_for_block(
                model_block,
                Some(&frozen),
                self.viewport_scale_handle(viewport.common.handle),
                self.annotation_all_visible(),
                false,
            ) {
                if let Some(wipeout) =
                    project_plot_fill(wipeout, &project_3d, xmin, ymin, xmax, ymax)
                {
                    projected_wipeouts.push(wipeout);
                }
            }
        }

        (
            pattern_wires,
            projected_hatches,
            projected_wipeouts,
            projected_images,
        )
    }

    /// A content viewport's clip boundary, projected into that viewport's
    /// render-target normalized device coords. Every content viewport goes
    /// through the same stencil path: a rectangular viewport contributes its
    /// own four corners (a full-target mask, since the render rectangle already
    /// clips the edges), a viewport with a `clip_boundary_handle` contributes
    /// that entity's tessellated outline. Returns empty only when the viewport
    /// is degenerate.
    ///
    /// `uo/vo/us/vs` are the visible-sub-rect crop the content camera applies
    /// (see `crop_view_proj`); the same crop is applied here so the boundary
    /// lines up with the content even when the viewport hangs off-canvas.
    pub(super) fn viewport_clip_boundary_ndc(
        &self,
        vp_handle: Handle,
        uo: f32,
        vo: f32,
        us: f32,
        vs: f32,
    ) -> Vec<[f32; 2]> {
        use document::entities::Viewport;
        let Some(EntityType::Viewport(vp)) = self.document.get_entity(vp_handle) else {
            return vec![];
        };
        let vp: &Viewport = vp;
        let pcx = vp.center.x as f32;
        let pcy = vp.center.y as f32;
        let z = vp.center.z as f32;
        let hw = (vp.width / 2.0) as f32;
        let hh = (vp.height / 2.0) as f32;
        if hw.abs() < 1e-6 || hh.abs() < 1e-6 {
            return vec![];
        }
        let poly = if !crate::entities::viewport::is_clipped(vp) {
            // Rectangular viewport → its own four corners (paper coords). These
            // map to full-rect NDC [-1, 1], a no-op mask over a render target
            // that already clips to the rectangle, but it keeps rect and
            // non-rect viewports on one uniform stencil path.
            vec![
                [pcx - hw, pcy - hh, z],
                [pcx + hw, pcy - hh, z],
                [pcx + hw, pcy + hh, z],
                [pcx - hw, pcy + hh, z],
            ]
        } else {
            self.clip_boundary_polygon(vp.clip_boundary_handle, z)
        };
        if poly.len() < 3 {
            return vec![];
        }
        // Paper point → full-rectangle NDC ([-1, 1] across the viewport rect,
        // Y up, matching the content camera + `viewport_screen_rect`), then the
        // same crop the content camera uses to map its visible sub-rect back to
        // NDC [-1, 1].
        let us = us.max(1e-6);
        let vs = vs.max(1e-6);
        let sx = 1.0 / us;
        let sy = 1.0 / vs;
        let tx = (1.0 - 2.0 * uo - us) / us;
        let ty = -(1.0 - 2.0 * vo - vs) / vs;
        poly.iter()
            .map(|p| {
                let nx = (p[0] - pcx) / hw;
                let ny = (p[1] - pcy) / hh;
                [sx * nx + tx, sy * ny + ty]
            })
            .collect()
    }

    /// Tessellated outline of a non-rectangular viewport / XCLIP clip-boundary
    /// entity, in paper coordinates. Closed polylines are sampled directly
    /// because their normal tessellation is a `Contour`, not `Lines`.
    pub(crate) fn clip_boundary_polygon(&self, handle: Handle, z: f32) -> Vec<[f32; 3]> {
        clip_boundary_polygon_for_document(&self.document, handle, z)
    }

    /// Plot wires for 3-D solids, surfaces and regions, which draw on screen as
    /// GPU meshes and so are absent from the wire set: their feature edges
    /// (triangle edges when a mesh has none) plus the silhouettes seen along
    /// `view`, in absolute WCS, coloured and weighted like their entity.
    pub(crate) fn mesh_plot_wires(
        &self,
        meshes: &[MeshLodSet],
        view: [f64; 3],
        viewport: Option<Handle>,
        shade: Option<document::entities::ViewportRenderMode>,
    ) -> Vec<WireModel> {
        use document::types::Vector3;
        let flags = shade.map(view::render::render_mode_flags);
        // Hidden and shaded modes hide the edges that faces cover; shaded
        // modes also fill the faces, and drop the edges unless the mode
        // shows them.
        let hidden = flags.is_some_and(|f| f.mesh_fill);
        let shaded = flags.is_some_and(|f| f.mesh_fill && !f.hidden_line);
        let edges = !shaded || flags.is_some_and(|f| f.show_3d_edges);
        let mut wires = if shaded {
            self.mesh_plot_faces(meshes, view)
        } else {
            Vec::new()
        };
        if !edges {
            return wires;
        }
        let occluders = hidden.then(|| Occluders::new(meshes, view));
        for set in meshes {
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            let place = |p: [f64; 3]| -> [f64; 3] {
                set.instance_transform.map_or(p, |xform| {
                    let q = xform.apply(Vector3::new(p[0], p[1], p[2]));
                    [q.x, q.y, q.z]
                })
            };
            let ds = |high: [f32; 3], low: Option<&[f32; 3]>| -> [f64; 3] {
                let low = low.copied().unwrap_or([0.0; 3]);
                std::array::from_fn(|i| high[i] as f64 + low[i] as f64)
            };
            let mut points: Vec<[f64; 3]> = Vec::new();
            let mut segment = |a: [f64; 3], b: [f64; 3]| {
                points.extend([place(a), place(b), [f64::NAN; 3]]);
            };
            let (edges, edges_low) = set.geometry_edges();
            if edges.len() >= 2 {
                for (i, pair) in edges.chunks_exact(2).enumerate() {
                    let a = ds(pair[0], edges_low.get(2 * i));
                    let b = ds(pair[1], edges_low.get(2 * i + 1));
                    segment(a, b);
                }
            } else if let Some(mesh) = set.geometry_lods().first() {
                let vertex = |i: u32| {
                    let i = i as usize;
                    ds(mesh.verts[i], mesh.verts_low.get(i))
                };
                for tri in mesh.indices.chunks_exact(3) {
                    for k in 0..3 {
                        segment(vertex(tri[k]), vertex(tri[(k + 1) % 3]));
                    }
                }
            }
            // Silhouettes of curved faces depend on the view, so they are not
            // in the edge list: a cylinder seen side-on needs its two outlines.
            let generators = set
                .instance_source
                .as_ref()
                .map_or(set.curved_gens.as_slice(), |source| {
                    source.curved_gens.as_slice()
                });
            for generator in generators {
                // A block instance's source is block-local: place it first, so
                // the contour is taken against the world view direction.
                let placed = set.instance_transform.and_then(|xform| {
                    let origin = xform.apply(Vector3::ZERO);
                    let axes = [Vector3::UNIT_X, Vector3::UNIT_Y, Vector3::UNIT_Z]
                        .map(|axis| xform.apply_rotation(axis))
                        .map(|v| [v.x, v.y, v.z]);
                    generator.source.transformed(axes, [origin.x, origin.y, origin.z])
                });
                if set.instance_transform.is_some() && placed.is_none() {
                    continue;
                }
                let source = placed.as_ref().unwrap_or(&generator.source);
                for pair in source.outline(view).chunks_exact(2) {
                    points.extend([pair[0], pair[1], [f64::NAN; 3]]);
                }
            }
            if let Some(occluders) = &occluders {
                points = points
                    .chunks_exact(3)
                    .flat_map(|seg| occluders.visible_parts(seg[0], seg[1]))
                    .collect();
            }
            if points.is_empty() {
                continue;
            }
            let color = set.display_color().unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let mut wire = WireModel::solid_f64(handle.value().to_string(), points, color, false);
            wire.effective_layer = set.effective_layer.clone();
            if let Some(entity) = self.document.get_entity(handle) {
                wire.line_weight_px =
                    view::render::render_style_for_viewport(&self.document, entity, viewport).3;
                wire.plot_visible = self
                    .document
                    .layers
                    .get(&entity.common().layer)
                    .is_none_or(|layer| layer.is_plottable);
            }
            wires.push(wire);
        }
        wires
    }

    /// The faces of every plotted mesh as filled triangles, shaded by how
    /// squarely they face the viewer and ordered far to near, so nearer
    /// faces paint over farther ones. Each triangle is its own wire (one
    /// colour per wire); all carry the depth of the lowest mesh in draw
    /// order, so they sort together under the edges.
    fn mesh_plot_faces(&self, meshes: &[MeshLodSet], view: [f64; 3]) -> Vec<WireModel> {
        use document::types::Vector3;
        let w = glam::DVec3::from(view).normalize_or(glam::DVec3::Z);
        let depths = self.draw_depth_map();
        let mut owner: Option<(f32, Handle)> = None;
        let mut faces: Vec<(f64, [glam::DVec3; 3], usize)> = Vec::new();
        let mut styles: Vec<([f32; 4], Option<std::sync::Arc<str>>)> = Vec::new();
        for set in meshes {
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            let Some(mesh) = set.geometry_lods().first() else {
                continue;
            };
            if let Some(entity) = self.document.get_entity(handle) {
                let plotted = self
                    .document
                    .layers
                    .get(&entity.common().layer)
                    .is_none_or(|layer| layer.is_plottable);
                if !plotted {
                    continue;
                }
            }
            let depth = depths.get(&handle.value()).map_or(0.0, |d| d[0]);
            if owner.is_none_or(|(d, _)| depth < d) {
                owner = Some((depth, handle));
            }
            let style = styles.len();
            styles.push((
                set.display_color().unwrap_or([0.7, 0.7, 0.7, 1.0]),
                set.effective_layer.clone(),
            ));
            let vertex = |i: u32| {
                let i = i as usize;
                let low = mesh.verts_low.get(i).copied().unwrap_or([0.0; 3]);
                let p: [f64; 3] = std::array::from_fn(|k| mesh.verts[i][k] as f64 + low[k] as f64);
                let p = set.instance_transform.map_or(p, |xform| {
                    let q = xform.apply(Vector3::new(p[0], p[1], p[2]));
                    [q.x, q.y, q.z]
                });
                glam::DVec3::from(p)
            };
            for tri in mesh.indices.chunks_exact(3) {
                let corners = [vertex(tri[0]), vertex(tri[1]), vertex(tri[2])];
                if !corners.iter().all(|c| c.is_finite()) {
                    continue;
                }
                let center = (corners[0] + corners[1] + corners[2]) / 3.0;
                faces.push((center.dot(w), corners, style));
            }
        }
        let Some((_, owner)) = owner else {
            return Vec::new();
        };
        faces.sort_by(|a, b| a.0.total_cmp(&b.0));
        let name = owner.value().to_string();
        faces
            .into_iter()
            .filter_map(|(_, corners, style)| {
                let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
                let normal = normal.try_normalize()?;
                // A headlight from the viewer: square faces full tone, edge-on
                // faces darkest.
                let light = 0.35 + 0.65 * normal.dot(w).abs() as f32;
                let (color, layer) = &styles[style];
                let shaded = [color[0] * light, color[1] * light, color[2] * light, color[3]];
                let mut wire = WireModel::solid(name.clone(), Vec::new(), shaded, false);
                let mut high = Vec::with_capacity(3);
                let mut low = Vec::with_capacity(3);
                for c in corners {
                    let split = c.to_array().map(WireModel::split_ds);
                    high.push(split.map(|(h, _)| h));
                    low.push(split.map(|(_, l)| l));
                }
                wire.fill_tris = high;
                wire.fill_tris_low = low;
                wire.fill_is_3d = true;
                wire.effective_layer = layer.clone();
                Some(wire)
            })
            .collect()
    }
}

/// The faces of every plotted mesh, for hiding the edges they cover when a
/// viewport plots hidden or shaded.
struct Occluders {
    /// Triangle corners, then the triangle's extent across the view
    /// (`[min_u, min_v, max_u, max_v]`) and its nearest depth toward the viewer.
    tris: Vec<([[f64; 3]; 3], [f64; 4], f64)>,
    axes: [glam::DVec3; 3],
    eps: f64,
}

impl Occluders {
    fn new(meshes: &[MeshLodSet], view: [f64; 3]) -> Self {
        use document::types::Vector3;
        let w = glam::DVec3::from(view).normalize_or(glam::DVec3::Z);
        let u = w.any_orthonormal_vector();
        let axes = [u, w.cross(u), w];
        let mut tris = Vec::new();
        let (mut lo, mut hi) = (glam::DVec3::splat(f64::MAX), glam::DVec3::splat(f64::MIN));
        for set in meshes {
            let Some(mesh) = set.geometry_lods().first() else {
                continue;
            };
            let vertex = |i: u32| {
                let i = i as usize;
                let low = mesh.verts_low.get(i).copied().unwrap_or([0.0; 3]);
                let p: [f64; 3] =
                    std::array::from_fn(|k| mesh.verts[i][k] as f64 + low[k] as f64);
                set.instance_transform.map_or(p, |xform| {
                    let q = xform.apply(Vector3::new(p[0], p[1], p[2]));
                    [q.x, q.y, q.z]
                })
            };
            for tri in mesh.indices.chunks_exact(3) {
                let corners = [vertex(tri[0]), vertex(tri[1]), vertex(tri[2])];
                let mut extent = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                let mut near = f64::MIN;
                for c in corners {
                    let c = glam::DVec3::from(c);
                    lo = lo.min(c);
                    hi = hi.max(c);
                    let (pu, pv) = (c.dot(axes[0]), c.dot(axes[1]));
                    extent = [
                        extent[0].min(pu),
                        extent[1].min(pv),
                        extent[2].max(pu),
                        extent[3].max(pv),
                    ];
                    near = near.max(c.dot(w));
                }
                tris.push((corners, extent, near));
            }
        }
        let eps = if tris.is_empty() {
            0.0
        } else {
            (hi - lo).length() * 1e-7
        };
        Self { tris, axes, eps }
    }

    /// Whether a face lies between `p` and the viewer.
    // ponytail: linear scan of every face per sample; bin the extents in a
    // grid if hidden-line plots of large models get slow.
    fn hidden(&self, p: glam::DVec3) -> bool {
        let [u, v, w] = self.axes;
        let (pu, pv, pw) = (p.dot(u), p.dot(v), p.dot(w));
        self.tris.iter().any(|(c, e, near)| {
            if *near <= pw + self.eps || pu < e[0] || pu > e[2] || pv < e[1] || pv > e[3] {
                return false;
            }
            // Ray from `p` toward the viewer against the triangle.
            let [a, b, c] = c.map(glam::DVec3::from);
            let (e1, e2) = (b - a, c - a);
            let h = w.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-18 {
                return false;
            }
            let s = p - a;
            let bu = s.dot(h) / det;
            if !(0.0..=1.0).contains(&bu) {
                return false;
            }
            let q = s.cross(e1);
            let bv = w.dot(q) / det;
            bv >= 0.0 && bu + bv <= 1.0 && e2.dot(q) / det > self.eps
        })
    }

    /// The pieces of `a`–`b` no face hides, as `[start, end, NaN]` triples.
    fn visible_parts(&self, a: [f64; 3], b: [f64; 3]) -> Vec<[f64; 3]> {
        let (a, b) = (glam::DVec3::from(a), glam::DVec3::from(b));
        if !a.is_finite() || !b.is_finite() {
            return Vec::new();
        }
        let n = (((b - a).length() / (self.eps * 1e4)).ceil() as usize).clamp(1, 64);
        let at = |i: usize| a.lerp(b, i as f64 / n as f64);
        let mut out = Vec::new();
        let mut start = None;
        for i in 0..n {
            let shown = !self.hidden(a.lerp(b, (i as f64 + 0.5) / n as f64));
            match (shown, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.extend([at(s).to_array(), at(i).to_array(), [f64::NAN; 3]]);
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.extend([at(s).to_array(), b.to_array(), [f64::NAN; 3]]);
        }
        out
    }
}

pub(crate) fn clip_boundary_polygon_for_document(
    document: &Document,
    handle: Handle,
    z: f32,
) -> Vec<[f32; 3]> {
    use std::f64::consts::TAU;
    let Some(entity) = document.get_entity(handle) else {
        return vec![];
    };
    // Circles and ellipses tessellate directly — their `to_render` returns a
    // parametric RenderObject (not `Lines`), so extracting a polygon there
    // would come back empty. Everything else (splines, polylines, …) reuses
    // the entity's own `Lines` tessellation.
    const N: usize = 64;
    match entity {
        EntityType::Circle(c) => (0..N)
            .map(|i| {
                let a = i as f64 * TAU / N as f64;
                [
                    (c.center.x + a.cos() * c.radius) as f32,
                    (c.center.y + a.sin() * c.radius) as f32,
                    z,
                ]
            })
            .collect(),
        EntityType::Ellipse(el) => {
            // major_axis = center → major endpoint; minor = perp(major) × ratio.
            let (mx, my) = (el.major_axis.x, el.major_axis.y);
            let r = el.minor_axis_ratio;
            (0..N)
                .map(|i| {
                    let t = i as f64 * TAU / N as f64;
                    let px = mx * t.cos() - my * r * t.sin();
                    let py = my * t.cos() + mx * r * t.sin();
                    [(el.center.x + px) as f32, (el.center.y + py) as f32, z]
                })
                .collect()
        }
        EntityType::LwPolyline(polyline) if polyline.is_closed => {
            let vertices: Vec<([f64; 2], f64)> = polyline
                .vertices
                .iter()
                .map(|vertex| ([vertex.location.x, vertex.location.y], vertex.bulge))
                .collect();
            sample_polyline_clip_boundary(
                &vertices,
                polyline.elevation,
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
                z,
            )
        }
        EntityType::Polyline2D(polyline) if polyline.is_closed() => {
            let vertices: Vec<([f64; 2], f64)> = polyline
                .vertices
                .iter()
                .map(|vertex| ([vertex.location.x, vertex.location.y], vertex.bulge))
                .collect();
            sample_polyline_clip_boundary(
                &vertices,
                polyline.elevation,
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
                z,
            )
        }
        EntityType::Polyline(polyline) if polyline.is_closed() => polyline
            .vertices
            .iter()
            .map(|vertex| [vertex.location.x as f32, vertex.location.y as f32, z])
            .collect(),
        EntityType::Polyline3D(polyline) if polyline.flags.closed => polyline
            .vertices
            .iter()
            .map(|vertex| [vertex.position.x as f32, vertex.position.y as f32, z])
            .collect(),
        // Anything else that draws as a plane curve — a circle, an ellipse, a
        // bulged polyline, a spline — is sampled through its own geometry,
        // which is where every entity's curve is defined once.
        _ => crate::entities::curve::entity_curve(entity)
            .map(|planar| crate::entities::curve::curve_points(&planar))
            .unwrap_or_default()
            .into_iter()
            .filter(|point| point[0].is_finite() && point[1].is_finite())
            .map(|point| [point[0] as f32, point[1] as f32, z])
            .collect(),
    }
}

fn sample_polyline_clip_boundary(
    vertices: &[([f64; 2], f64)],
    elevation: f64,
    normal: (f64, f64, f64),
    z: f32,
) -> Vec<[f32; 3]> {
    if vertices.len() < 3 {
        return Vec::new();
    }
    let to_wcs = |point: [f64; 2]| {
        crate::scene::view::transform::ocs_point_to_wcs((point[0], point[1], elevation), normal)
    };
    let mut output = Vec::new();
    let first = to_wcs(vertices[0].0);
    output.push([first.0 as f32, first.1 as f32, z]);
    for index in 0..vertices.len() {
        let (start, bulge) = vertices[index];
        let end_index = (index + 1) % vertices.len();
        let end = vertices[end_index].0;
        if let Some(arc) = crate::entities::common::BulgeArc::from_bulge(start, end, bulge) {
            let steps =
                ((arc.sweep.abs() / std::f64::consts::TAU * 64.0).ceil() as usize).clamp(4, 64);
            for step in 1..=steps {
                if end_index == 0 && step == steps {
                    break;
                }
                let point = to_wcs(arc.sample(step as f64 / steps as f64));
                output.push([point.0 as f32, point.1 as f32, z]);
            }
        } else if end_index != 0 {
            let point = to_wcs(end);
            output.push([point.0 as f32, point.1 as f32, z]);
        }
    }
    output
}

fn project_plot_fill<F>(
    mut fill: HatchModel,
    project: &F,
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
) -> Option<HatchModel>
where
    F: Fn([f64; 3]) -> Option<[f32; 2]>,
{
    let mut output = Vec::new();
    let mut ring = Vec::new();
    let flush_ring = |ring: &mut Vec<[f32; 2]>, output: &mut Vec<[f32; 2]>| {
        if ring.len() < 3 {
            ring.clear();
            return;
        }
        if ring.first() == ring.last() {
            ring.pop();
        }
        let mut clipped = clip_polygon_to_rect(ring, xmin, ymin, xmax, ymax);
        ring.clear();
        if clipped.len() < 3 {
            return;
        }
        if clipped.first() != clipped.last() {
            clipped.push(clipped[0]);
        }
        if !output.is_empty() {
            output.push([f32::NAN, f32::NAN]);
        }
        output.extend(clipped);
    };
    if let (Some(plane), Some(boundary)) = (fill.fill_plane, fill.fill_plane_boundary.as_deref()) {
        let plane = ocs_geom::space::Plane::from_axes(plane.origin, plane.x_axis, plane.y_axis);
        for &[x, y] in boundary {
            if x.is_nan() || y.is_nan() {
                flush_ring(&mut ring, &mut output);
                continue;
            }
            let point = plane.point_at([x as f64, y as f64]);
            if let Some(point) = project(point) {
                ring.push(point);
            }
        }
    } else {
        for &[x, y] in fill.boundary.iter() {
            if x.is_nan() || y.is_nan() {
                flush_ring(&mut ring, &mut output);
                continue;
            }
            if let Some(point) = project([
                fill.world_origin[0] + x as f64,
                fill.world_origin[1] + y as f64,
                0.0,
            ]) {
                ring.push(point);
            }
        }
    }
    flush_ring(&mut ring, &mut output);
    if output.is_empty() {
        return None;
    }
    fill.world_origin = [0.0, 0.0];
    fill.boundary = std::sync::Arc::new(output);
    fill.boundary_wcs = None;
    fill.fill_plane = None;
    fill.fill_plane_boundary = None;
    Some(fill)
}

fn clip_polygon_to_rect(
    polygon: &[[f32; 2]],
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
) -> Vec<[f32; 2]> {
    let mut output = polygon.to_vec();
    for (edge, value) in [(0u8, xmin), (1, xmax), (2, ymin), (3, ymax)] {
        if output.is_empty() {
            break;
        }
        let input = std::mem::take(&mut output);
        let mut previous = *input.last().unwrap();
        let mut previous_inside = polygon_edge_inside(previous, edge, value);
        for current in input {
            let current_inside = polygon_edge_inside(current, edge, value);
            if current_inside != previous_inside {
                output.push(polygon_edge_intersection(previous, current, edge, value));
            }
            if current_inside {
                output.push(current);
            }
            previous = current;
            previous_inside = current_inside;
        }
    }
    output
}

fn polygon_edge_inside(point: [f32; 2], edge: u8, value: f32) -> bool {
    match edge {
        0 => point[0] >= value,
        1 => point[0] <= value,
        2 => point[1] >= value,
        _ => point[1] <= value,
    }
}

fn polygon_edge_intersection(start: [f32; 2], end: [f32; 2], edge: u8, value: f32) -> [f32; 2] {
    if edge <= 1 {
        let dx = end[0] - start[0];
        let t = if dx.abs() > 1e-12 {
            (value - start[0]) / dx
        } else {
            0.0
        };
        [value, start[1] + (end[1] - start[1]) * t]
    } else {
        let dy = end[1] - start[1];
        let t = if dy.abs() > 1e-12 {
            (value - start[1]) / dy
        } else {
            0.0
        };
        [start[0] + (end[0] - start[0]) * t, value]
    }
}

// ── Paper boundary wire ────────────────────────────────────────────────────

// ── Cohen-Sutherland line clipping ───────────────────────────────────────

/// Clip a single segment (x0,y0)→(x1,y1) against the axis-aligned rectangle
/// [xmin,xmax]×[ymin,ymax].  Returns the clipped endpoints or `None` if the
/// segment is entirely outside.
fn cs_clip(
    mut x0: f32,
    mut y0: f32,
    mut x1: f32,
    mut y1: f32,
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
) -> Option<(f32, f32, f32, f32)> {
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;

    let code = |x: f32, y: f32| -> u8 {
        let mut c = 0u8;
        if x < xmin {
            c |= LEFT;
        } else if x > xmax {
            c |= RIGHT;
        }
        if y < ymin {
            c |= BOTTOM;
        } else if y > ymax {
            c |= TOP;
        }
        c
    };

    let mut c0 = code(x0, y0);
    let mut c1 = code(x1, y1);

    loop {
        if c0 | c1 == 0 {
            return Some((x0, y0, x1, y1));
        }
        if c0 & c1 != 0 {
            return None;
        }
        let cout = if c0 != 0 { c0 } else { c1 };
        let (x, y);
        if cout & TOP != 0 {
            x = x0 + (x1 - x0) * (ymax - y0) / (y1 - y0);
            y = ymax;
        } else if cout & BOTTOM != 0 {
            x = x0 + (x1 - x0) * (ymin - y0) / (y1 - y0);
            y = ymin;
        } else if cout & RIGHT != 0 {
            y = y0 + (y1 - y0) * (xmax - x0) / (x1 - x0);
            x = xmax;
        } else {
            y = y0 + (y1 - y0) * (xmin - x0) / (x1 - x0);
            x = xmin;
        }
        if cout == c0 {
            x0 = x;
            y0 = y;
            c0 = code(x0, y0);
        } else {
            x1 = x;
            y1 = y;
            c1 = code(x1, y1);
        }
    }
}

/// Clip a projected polyline (NaN-separated segments) to the viewport rectangle.
/// Returns a new points vec with proper NaN separators at clip boundaries.
/// Cut a projected polyline (each point with its depth toward the eye) to
/// the depths `back..=front`, breaking it with NaN where it leaves them.
/// Crossings interpolate linearly, which is exact for a parallel projection.
fn clip_polyline_to_depth(pts: &[([f32; 3], f64)], back: f64, front: f64) -> Vec<[f32; 3]> {
    const NAN3: [f32; 3] = [f32::NAN, f32::NAN, f32::NAN];
    let lerp = |a: [f32; 3], b: [f32; 3], t: f64| {
        let t = t as f32;
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    let mut out: Vec<[f32; 3]> = Vec::new();
    let mut open = false;
    for pair in pts.windows(2) {
        let ((a, da), (b, db)) = (pair[0], pair[1]);
        if !a[0].is_finite() || !b[0].is_finite() {
            if open {
                out.push(NAN3);
                open = false;
            }
            continue;
        }
        // Parametric extent of the segment inside the slab.
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        for (limit, below) in [(back, true), (front, false)] {
            if !limit.is_finite() {
                continue;
            }
            let (sa, sb) = if below {
                (da - limit, db - limit)
            } else {
                (limit - da, limit - db)
            };
            if sa < 0.0 && sb < 0.0 {
                t0 = 1.0;
                t1 = 0.0;
            } else if sa < 0.0 {
                t0 = t0.max(sa / (sa - sb));
            } else if sb < 0.0 {
                t1 = t1.min(sa / (sa - sb));
            }
        }
        if t0 > t1 {
            if open {
                out.push(NAN3);
                open = false;
            }
            continue;
        }
        if !open || t0 > 0.0 {
            if open {
                out.push(NAN3);
            }
            out.push(lerp(a, b, t0));
        }
        out.push(lerp(a, b, t1));
        open = t1 >= 1.0;
        if !open {
            out.push(NAN3);
        }
    }
    while out.last().is_some_and(|p| !p[0].is_finite()) {
        out.pop();
    }
    out
}

fn clip_polyline_to_rect(
    pts: &[[f32; 3]],
    stations: Option<&[f32]>,
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
    z: f32,
) -> (Vec<[f32; 3]>, Vec<f32>) {
    const NAN3: [f32; 3] = [f32::NAN, f32::NAN, f32::NAN];
    let mut result: Vec<[f32; 3]> = Vec::new();
    let mut result_stations = Vec::new();
    let mut i = 0;

    while i < pts.len() {
        // Skip NaN separators.
        if pts[i][0].is_nan() || pts[i][1].is_nan() {
            i += 1;
            continue;
        }
        // Gather contiguous run of finite points.
        let start = i;
        while i < pts.len() && pts[i][0].is_finite() && pts[i][1].is_finite() {
            i += 1;
        }
        let seg = &pts[start..i];
        if seg.len() < 2 {
            continue;
        }

        // Clip each edge and track pen state to insert NaN on lift.
        let mut pen_down = false;
        for j in 0..seg.len() - 1 {
            let [x0, y0, _] = seg[j];
            let [x1, y1, _] = seg[j + 1];
            match cs_clip(x0, y0, x1, y1, xmin, ymin, xmax, ymax) {
                None => {
                    pen_down = false;
                }
                Some((cx0, cy0, cx1, cy1)) => {
                    let parameter = |x: f32, y: f32| {
                        let dx = x1 - x0;
                        let dy = y1 - y0;
                        if dx.abs() >= dy.abs() && dx.abs() > 1e-12 {
                            (x - x0) / dx
                        } else if dy.abs() > 1e-12 {
                            (y - y0) / dy
                        } else {
                            0.0
                        }
                    };
                    let station = |t: f32| {
                        stations.map_or(0.0, |values| {
                            let start_station = values[start + j];
                            start_station + (values[start + j + 1] - start_station) * t
                        })
                    };
                    if !pen_down {
                        if !result.is_empty() {
                            result.push(NAN3);
                            if stations.is_some() {
                                result_stations.push(0.0);
                            }
                        }
                        result.push([cx0, cy0, z]);
                        if stations.is_some() {
                            result_stations.push(station(parameter(cx0, cy0)));
                        }
                        pen_down = true;
                    } else if let Some(&[lx, ly, _]) = result.last() {
                        if (lx - cx0).abs() > 1e-4 || (ly - cy0).abs() > 1e-4 {
                            result.push(NAN3);
                            result.push([cx0, cy0, z]);
                            if stations.is_some() {
                                result_stations.push(0.0);
                                result_stations.push(station(parameter(cx0, cy0)));
                            }
                        }
                    }
                    result.push([cx1, cy1, z]);
                    if stations.is_some() {
                        result_stations.push(station(parameter(cx1, cy1)));
                    }
                    // If the exit point was clipped, lift pen.
                    if (cx1 - x1).abs() > 1e-4 || (cy1 - y1).abs() > 1e-4 {
                        pen_down = false;
                    }
                }
            }
        }
    }
    // Remove trailing NaN.
    while result
        .last()
        .map(|p: &[f32; 3]| p[0].is_nan())
        .unwrap_or(false)
    {
        result.pop();
        if stations.is_some() {
            result_stations.pop();
        }
    }
    (result, result_stations)
}
