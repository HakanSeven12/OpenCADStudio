//! How a multileader draws: leaders, landing, arrowheads, its text or block
//! content and frame, in the scale and viewport it is shown in.

use document::entities::MultiLeader;

use crate::entities::multileader::*;
use crate::entities::text_support::MTextRenderOpts;
use crate::entities::text_support::layout_mtext;
use crate::entities::text_support::resolve_text_style;
use crate::display::wire_model::TangentGeom;
use document::entities::LeaderContentType;
use document::entities::MultiLeaderPathType;
use glam::DVec3;

/// Per-entity tessellation entry for `MultiLeader`. Returns multiple
/// `WireModel`s (leader/dogleg + arrow fill, optional block content,
/// text strokes, frame, background fill) so each piece can carry its
/// own colour.
pub trait MultiLeaderTess {
    fn tessellate(
        &self,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<document::Handle>,
        annotation_scale_handle: Option<document::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
        complex_lt_anno: f32,
    ) -> Vec<crate::display::wire_model::WireModel>;
}

impl MultiLeaderTess for MultiLeader {
    fn tessellate(
        &self,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<document::Handle>,
        annotation_scale_handle: Option<document::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
        complex_lt_anno: f32,
    ) -> Vec<crate::display::wire_model::WireModel> {
        // The leader, text and mask are laid out in f32. At large absolute
        // coordinates (survey drawings far from the origin) f32 steps grow to
        // tens of units, so a label collapses: its mask shrinks to nothing and
        // its text and leader snap to rounded points. Build everything around
        // the text location instead, then lift the wires back to world
        // double-single, like the MLINE complex-linetype path does.
        let o = self.context.text_location;
        let origin = [o.x, o.y, o.z];
        let mut local = self.clone();
        local.translate(document::types::Vector3::new(-o.x, -o.y, -o.z));
        let (mut wires, world_wires) = local.tessellate_in_frame(
            self,
            document,
            handle,
            selected,
            entity_color,
            line_weight_px,
            anno_scale,
            active_viewport,
            annotation_scale_handle,
            block_cache,
            view_aabb,
            world_per_pixel,
            bg_color,
            complex_lt_anno,
        );
        for (i, wire) in wires.iter_mut().enumerate() {
            if !world_wires.contains(&i) {
                lift_wire_to_world(wire, origin);
            }
        }
        wires
    }
}

/// Shift a wire built around `origin` back to world coordinates: everything
/// `shift_wire_to_world` covers plus the fills, pick triangles, snap points,
/// key vertices and tangents a MULTILEADER emits.
fn lift_wire_to_world(w: &mut crate::display::wire_model::WireModel, origin: [f64; 3]) {
    use crate::scene::convert::tessellate::{shift_wire_to_world, split_ds_xyz};
    shift_wire_to_world(w, origin);
    let lift = |tris: &mut Vec<[f32; 3]>, low: &mut Vec<[f32; 3]>| {
        if tris.is_empty() {
            return;
        }
        let mut hi = Vec::with_capacity(tris.len());
        let mut lo = Vec::with_capacity(tris.len());
        for (i, p) in tris.iter().enumerate() {
            let l = low.get(i).copied().unwrap_or([0.0; 3]);
            let (h, l) = split_ds_xyz(
                p[0] as f64 + l[0] as f64 + origin[0],
                p[1] as f64 + l[1] as f64 + origin[1],
                p[2] as f64 + l[2] as f64 + origin[2],
            );
            hi.push(h);
            lo.push(l);
        }
        *tris = hi;
        *low = lo;
    };
    let geom = w.geom_mut();
    if let Some(surfaces) = geom.surfaces.as_deref_mut() {
        lift(&mut surfaces.fill_tris, &mut surfaces.fill_tris_low);
        lift(&mut surfaces.pick_tris, &mut surfaces.pick_tris_low);
    }
    let offset = DVec3::from_array(origin);
    for (p, _) in &mut w.snap_pts {
        *p += offset;
    }
    for v in &mut w.key_vertices {
        for k in 0..3 {
            v[k] += origin[k];
        }
    }
    for run in &mut w.searchable_text {
        for k in 0..3 {
            run.origin[k] += origin[k];
        }
    }
    for tangent in &mut w.tangent_geoms {
        if let TangentGeom::Line { p1, p2 } = tangent {
            for p in [p1, p2] {
                for k in 0..3 {
                    p[k] = (p[k] as f64 + origin[k]) as f32;
                }
            }
        }
    }
}

/// Tessellation of a MULTILEADER already moved into a local frame.
trait MultiLeaderFrameTess {
    /// `world` is the same leader at its real position: block content is
    /// expanded from it, since the block path keeps its own precision.
    /// Returns the wires and the range of those already in world coordinates.
    fn tessellate_in_frame(
        &self,
        world: &MultiLeader,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<document::Handle>,
        annotation_scale_handle: Option<document::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
        complex_lt_anno: f32,
    ) -> (
        Vec<crate::display::wire_model::WireModel>,
        std::ops::Range<usize>,
    );
}

impl MultiLeaderFrameTess for MultiLeader {
    fn tessellate_in_frame(
        &self,
        world: &MultiLeader,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<document::Handle>,
        annotation_scale_handle: Option<document::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
        complex_lt_anno: f32,
    ) -> (
        Vec<crate::display::wire_model::WireModel>,
        std::ops::Range<usize>,
    ) {
        use crate::scene::convert::tessellate::{
            append_arrow, arrow_from_block, color_or_inherit, ArrowKind, DimGeom,
        };
        use crate::display::wire_model::{SnapHint, TangentGeom, WireModel};
        use glam::Vec3;
        let ml = self;
        // line_color falls back to the entity colour when the MultiLeader's own
        // colour is ByBlock/ByLayer; otherwise the leader uses its stored hue.
        let line_color = if selected {
            WireModel::SELECTED
        } else {
            color_or_inherit(&ml.line_color, entity_color)
        };
        // ml.line_weight is the leader-line weight override. Negative codes
        // (ByLayer/ByBlock/Default) fall through to the entity's already-resolved
        // pixel width.
        let leader_lw_px = match ml.line_weight {
            document::types::LineWeight::Value(v) if v >= 0 => (v as f32 / 100.0) * (96.0 / 25.4),
            _ => line_weight_px,
        };
        // ml.line_type_handle — resolve via line_types table by handle and apply
        // the resulting dash pattern to the leader wire.
        let lt_scale = document.header.linetype_scale as f32 * ml.common.linetype_scale as f32;
        let (leader_pat_len, leader_pat) = match ml.line_type_handle {
            Some(h) if !h.is_null() => {
                let name = document
                    .line_types
                    .iter()
                    .find(|lt| lt.handle == h)
                    .map(|lt| lt.name.clone());
                match name {
                    Some(n) => crate::scene::view::render::resolve_pattern(
                        &document.line_types,
                        &n,
                        lt_scale,
                    ),
                    None => (0.0, [0.0; 8]),
                }
            }
            _ => (0.0, [0.0; 8]),
        };

        let name = handle.value().to_string();
        let nan = [f32::NAN; 3];
        let p3 = |v: &document::types::Vector3| -> [f32; 3] {
            [(v.x) as f32, (v.y) as f32, (v.z) as f32]
        };

        // ── Scaling ──────────────────────────────────────────────────────────────
        // Used only when a context omits an already-resolved content size.
        let annotative = crate::entities::annotative::mleader_is_annotative(document, ml);

        // Context sizes are resolved at their stored annotation scale.
        let base_scale = if ml.scale_factor.abs() > 1.0e-12 {
            ml.scale_factor as f32
        } else {
            1.0
        };

        let stored_context_scale = ml.context.scale_factor as f32;

        // An annotative MLEADER ignores its overall scale (greyed out in the
        // properties panel); only the annotation scale resizes the context.
        let context_scale_correction = if annotative && stored_context_scale.abs() > 1.0e-12 {
            let correction = anno_scale / stored_context_scale;

            if correction.is_finite() && correction > 0.0 {
                correction
            } else {
                1.0
            }
        } else {
            1.0
        };

        let fallback_content_scale = if annotative { anno_scale } else { base_scale };

        // The active context stores the resolved world-space arrow size.
        // Reapplying the entity scale here makes context-sized arrows grow twice.
        let arrow_size = ml.context.arrowhead_size as f32 * context_scale_correction;
        let draw_arrow = arrow_size > 0.0;
        let invisible = ml.path_type == MultiLeaderPathType::Invisible;
        // arrowhead_handle resolves through the block records to a named arrow
        // block (matches DIMLDRBLK on Dimension). Null handle / unknown name →
        // ClosedFilled triangle.
        let arrow_kind = match ml.arrowhead_handle {
            Some(h) if !h.is_null() => arrow_from_block(document, h, arrow_size.max(0.001)),
            _ => ArrowKind::Triangle {
                size: arrow_size.max(0.001),
                filled: true,
                size_mul: 1.0,
            },
        };

        // ── Leader / arrow / dogleg points ───────────────────────────────────────
        let mut points: Vec<[f32; 3]> = Vec::new();
        let mut key_verts: Vec<[f64; 3]> = Vec::new();
        let mut snap_pts: Vec<(glam::DVec3, SnapHint)> = Vec::new();
        let mut tangents: Vec<TangentGeom> = Vec::new();
        let mut arrow_fill: Vec<[f32; 3]> = Vec::new();
        let mut first = true;

        // Which side the text grip sits on, recomputed every frame so the text
        // alignment and the landing mirror live when the arrow or text moves.
        let mut text_loc_w = ml.context.text_location;
        // Where the first root's landing (dogleg) ends; an underline starts here.
        let mut landing_end_w: Option<[f32; 2]> = None;

        // Scale the annotation-side offset around the fixed elbow.
        if annotative && (context_scale_correction - 1.0).abs() > 1.0e-6 {
            if let Some(root) = ml.context.leader_roots.first() {
                let anchor = root.connection_point;
                let k = context_scale_correction as f64;

                text_loc_w = document::types::Vector3::new(
                    anchor.x + (text_loc_w.x - anchor.x) * k,
                    anchor.y + (text_loc_w.y - anchor.y) * k,
                    text_loc_w.z,
                );
            }
        }
        let leader_ref_w = ml
            .context
            .leader_roots
            .first()
            .and_then(|r| r.lines.first())
            .and_then(|l| l.points.last())
            .copied()
            .or_else(|| ml.context.leader_roots.first().map(|r| r.connection_point))
            .unwrap_or(text_loc_w);
        let text_sign_w: f64 = if text_loc_w.x >= leader_ref_w.x {
            1.0
        } else {
            -1.0
        };

        for root in &ml.context.leader_roots {
            let cp = &root.connection_point;
            let cp_f = p3(cp);
            snap_pts.push((Vec3::from(cp_f).as_dvec3(), SnapHint::Node));

            for line in &root.lines {
                if line.points.is_empty() {
                    continue;
                }
                let mut curve_start: Option<[f64; 3]> = None;

                if !invisible {
                    if !first {
                        points.push(nan);
                    }
                    first = false;

                    let mut ctrl: Vec<[f32; 3]> = line.points.iter().map(&p3).collect();
                    let last_f = *ctrl.last().unwrap_or(&cp_f);
                    let dist =
                        ((last_f[0] - cp_f[0]).powi(2) + (last_f[1] - cp_f[1]).powi(2)).sqrt();
                    if dist > 1e-9 {
                        ctrl.push(cp_f);
                    }
                    for &c in &ctrl {
                        key_verts.push([c[0] as f64, c[1] as f64, c[2] as f64]);
                        snap_pts.push((Vec3::from(c).as_dvec3(), SnapHint::Node));
                    }

                    if line_path_type(ml, line) == MultiLeaderPathType::Spline && ctrl.len() >= 2 {
                        let mut ctrl_f64: Vec<[f64; 3]> = ctrl
                            .iter()
                            .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
                            .collect();
                        let landing = landing_direction(root).unwrap_or([text_sign_w, 0.0]);
                        // The curve runs on to the landing's text end; it has
                        // no separate landing.
                        if draws_landing(ml, root) {
                            let d = root.landing_distance * context_scale_correction as f64;
                            *ctrl_f64.last_mut().unwrap() =
                                [cp.x + landing[0] * d, cp.y + landing[1] * d, cp.z];
                        }
                        let curve = spline_into_landing(&ctrl_f64, landing);
                        curve_start = curve.get(1).copied();
                        for pt in curve {
                            points.push([pt[0] as f32, pt[1] as f32, pt[2] as f32]);
                        }
                    } else {
                        for &c in &ctrl {
                            points.push(c);
                        }
                    }
                    for i in 0..ctrl.len().saturating_sub(1) {
                        tangents.push(TangentGeom::Line {
                            p1: ctrl[i],
                            p2: ctrl[i + 1],
                        });
                    }
                }

                // A leader of type None draws neither its arrowhead nor its
                // landing: only the content shows.
                if draw_arrow && !invisible {
                    let tip = &line.points[0];
                    let tip_f = p3(tip);
                    // A curve leaves the tip along its own first step.
                    let next = match curve_start {
                        Some(c) => document::types::Vector3::new(c[0], c[1], c[2]),
                        None if line.points.len() >= 2 => line.points[1],
                        None => *cp,
                    };
                    let dx = (next.x - tip.x) as f32;
                    let dy = (next.y - tip.y) as f32;
                    let dl = (dx * dx + dy * dy).sqrt().max(1e-9);
                    let dir = Vec3::new(dx / dl, dy / dl, 0.0);
                    let tip_v = Vec3::new(tip_f[0], tip_f[1], tip_f[2]);
                    // Reuse the dim/leader arrow emitter so MultiLeader's arrow
                    // matches the block referenced by arrowhead_handle.
                    let mut arrow_geom = DimGeom::new();
                    append_arrow(&mut arrow_geom, tip_v, dir, &arrow_kind);
                    if !arrow_geom.dim_lines.is_empty() {
                        points.push(nan);
                        points.extend(arrow_geom.dim_lines);
                    }
                    arrow_fill.extend(arrow_geom.arrow_fill);
                }
            }

            if !invisible && draws_landing(ml, root) {
                // Horizontal landing (dogleg) from the leader elbow (connection
                // point) toward the text side. The stored geometry places the
                // text so its near-bottom edge sits one landing_gap past this
                // dogleg end, so the dogleg stops here — drawing on to
                // text_location (the block's top-left insertion) would streak a
                // stray line up the side of the text.
                // Landing distance belongs to the selected leader-root context
                // and is already resolved in world units.
                let d = root.landing_distance * context_scale_correction as f64;
                // The dogleg runs along the leader root's stored direction —
                // for a rotated leader that is the angled baseline, not world
                // X. Roots without a usable direction keep the legacy
                // horizontal toward the text side.
                let (ux, uy) = Some((root.direction.x, root.direction.y))
                    .filter(|(x, y)| (x * x + y * y).sqrt() > 1e-9)
                    .map(|(x, y)| {
                        let l = (x * x + y * y).sqrt();
                        (x / l, y / l)
                    })
                    .unwrap_or((text_sign_w, 0.0));
                let landing_end = [(cp.x + ux * d) as f32, (cp.y + uy * d) as f32, cp_f[2]];
                landing_end_w.get_or_insert([landing_end[0], landing_end[1]]);
                points.push(nan);
                if !spline_root(ml, root) {
                    points.push(cp_f);
                }
                points.push(landing_end);
                // A root landing on the text's other side than the first
                // root meets the text's far edge: it never runs on.
                let opposite_first = ml.context.leader_roots.first().is_some_and(|first| {
                    first.direction.x * root.direction.x + first.direction.y * root.direction.y
                        < 0.0
                });
                if ml.extend_leader_to_text && !opposite_first {
                    // Run on along the landing up to one gap short of the
                    // text. text_location is the text block's corner, often
                    // above the landing, so only its reach along the landing
                    // counts — aiming at the corner itself drew a stray line
                    // up the side of the text.
                    let gap = ml.context.landing_gap.max(0.0) * context_scale_correction as f64;
                    let reach = (text_loc_w.x - cp.x) * ux + (text_loc_w.y - cp.y) * uy - gap;
                    if reach > d {
                        points.push([
                            (cp.x + ux * reach) as f32,
                            (cp.y + uy * reach) as f32,
                            cp_f[2],
                        ]);
                    }
                }
            }
        }

        // The leader/arrow/dogleg wire goes out as a single WireModel. Text, frame,
        // and background fill (each with their own color) are appended as separate
        // WireModels so the renderer respects per-piece coloring.
        let mut wires: Vec<WireModel> = Vec::new();
        wires.push(WireModel {
            bg_adapt: None,
            point_marker: None,
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            snap_only: false,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            solid_normal: None,
            render_instance: None,
            dash_from_start: false,
            name: name.clone(),
            color: line_color,
            selected,
            aci: 0,
            pattern_length: leader_pat_len,
            pattern: leader_pat,
            line_weight_px: leader_lw_px,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            geom: std::sync::Arc::new(crate::display::wire_model::WireGeom {
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                text_verts: Vec::new(),
                points,
                points_low: Default::default(),
                snap_pts,
                tangent_geoms: tangents,
                key_vertices: key_verts,
                // fill_tris_low intentionally empty: this fill renders on the
                // top-level path, where consumers (face3d_gpu, xclip) treat a short
                // low half as all-zero, so it draws at f32 precision (sub-metre
                // error at UTM scale) — not a crash. Follow-up: double-single-split
                // via points_to_ds to match emit_wire's paired fill path.
                surfaces: crate::display::wire_model::WireSurfaces {
                    fill_tris: arrow_fill,
                    ..Default::default()
                }
                .into_optional(),
                ..crate::display::wire_model::WireGeom::default()
            }),
            ..Default::default()
        });

        let world_start = wires.len();
        let host = document::EntityType::MultiLeader(Box::new(world.clone()));
        for block_use in crate::scene::render_graph::entity_render_block_uses(document, &host, 1.0)
            .into_iter()
            .filter(|block_use| {
                block_use.active
                    && block_use.role == crate::scene::render_graph::BlockRole::MultiLeaderContent
            })
        {
            let expanded = crate::scene::convert::tess::expand_block_object(
                document,
                &block_use.insert,
                handle,
                selected,
                active_viewport,
                bg_color,
                1.0,
                annotation_scale_handle,
                block_cache,
                view_aabb,
                world_per_pixel,
                1.0,
                crate::scene::convert::tess::BlockObjectOptions {
                    scale_policy: block_use.scale_policy,
                    complex_lt_anno: Some(complex_lt_anno),
                    ..crate::scene::convert::tess::BlockObjectOptions::default()
                },
            );
            let style = expanded.style;
            wires.extend(expanded.wires);
            crate::scene::convert::insert_tess::append_insert_attribute_wires(
                &mut wires,
                document,
                &block_use.insert,
                handle,
                selected,
                style.color,
                style.pattern_length,
                style.pattern,
                style.line_weight_px,
                style.layer,
                style.layer_plottable,
                bg_color,
                expanded.is_xref,
                1.0,
                1.0,
                annotation_scale_handle,
            );
        }
        let world_wires = world_start..wires.len();

        // ── Text strokes / frame / background fill ──────────────────────────────
        // Strip inline format codes, split / word-wrap into lines, then place each
        // line according to text_attachment_point (horizontal) and
        // text_left_attachment (vertical), with text_rotation/text_direction applied.
        if ml.content_type == LeaderContentType::MText && !ml.context.text_string.is_empty() {
            let ctx = &ml.context;
            // `ctx.text_height` (when > 0) is the already-resolved WCS text height
            // stored in the per-instance context — style × scale_factor × anno
            // scale are all already baked in. Multiplying by `effective_scale`
            // again would double-scale (e.g., a context height of 100 in a file
            // with scale_factor=20 was rendering at 2000 units — 20× too big).
            // Only the fallback path (file omits the context value) needs
            // scale_factor + annotation scale applied.
            let height = if ctx.text_height > 0.0 {
                ctx.text_height as f32 * context_scale_correction
            } else {
                ml.text_height as f32 * fallback_content_scale
            };

            let ins = &text_loc_w;
            // Subtract world_offset in f64 before casting to f32: drawings often
            // sit at large absolute coordinates and casting first then subtracting
            // throws away the precision needed for the rotated sub-glyph offsets.
            let local_ins_x = (ins.x) as f32;
            let z = (ins.z) as f32;

            // The context stores the resolved display rotation.
            let td = ctx.text_direction;
            let mut rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
                (td.y as f32).atan2(td.x as f32)
            } else {
                ctx.text_rotation as f32
            };
            if ml.text_direction_negative {
                rot += std::f32::consts::PI;
            }

            let vertical_attach = active_vertical_attachment(ml);

            // Resolve text style via handle when available, falling back to STANDARD.
            let style_name = ctx
                .text_style_handle
                .as_ref()
                .and_then(|h| {
                    document
                        .text_styles
                        .iter()
                        .find(|s| s.handle == *h)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| "STANDARD".to_string());
            let style = resolve_text_style(&style_name, document);
            if style.is_upside_down {
                rot += std::f32::consts::PI;
            }
            let (cos_r, sin_r) = (rot.cos(), rot.sin());

            // The stored text attachment point is the insertion's horizontal
            // anchor within the text block (Left/Center/Right) — honour it
            // instead of guessing from the leader side.
            use document::entities::multileader::TextAttachmentDirectionType;
            // The context's attachment point exists in every DWG version;
            // the entity-level copy only exists from R2010 on.
            let h_anchor: f32 = attachment_column(ctx.text_attachment_point) as f32 * 0.5;
            // Pick the vertical-anchor attachment based on text_attachment_direction:
            //   Horizontal — leader attaches left/right; use ml.text_left_attachment
            //                (matches the file's stored ctx.text_left_attachment).
            //   Vertical   — leader attaches top/bottom; use ml.text_top_attachment
            //                or ml.text_bottom_attachment depending on which side
            //                the leader is coming from (chosen via the first
            //                root.direction.y sign).
            let v_anchor = mleader_v_anchor(vertical_attach);

            // The anchor snap needs the block's width along the reading axis,
            // which only the layout knows. Line metrics do not depend on the
            // insertion point, so a first pass with the raw anchor measures
            // them; the final pass below places the strokes.
            let mtext_opts = MTextRenderOpts {
                layer_color: None,
                // Not an MTEXT: text in a fixed box, never columnar.
                columns: Default::default(),
                value: &ctx.text_string,
                insertion: [ins.x, ins.y, z as f64],
                height,
                rect_w: ctx.text_width as f32 * context_scale_correction,
                rotation: rot,
                style: &style,
                attach_h_anchor: h_anchor,
                v_anchor,
                line_spacing_factor: ctx.line_spacing_factor as f32,
                exact_line_spacing: matches!(
                    ctx.line_spacing_style,
                    document::entities::LineSpacingStyle::Exactly
                ),
                rectangle_height: 0.0,
                // Vertical flow (top-to-bottom) is stored per-context.
                vertical_text: matches!(
                    ctx.text_flow_direction,
                    document::entities::multileader::FlowDirectionType::Vertical
                ),
                want_glyph_boxes: false,
                break_long_words: false,
            };
            let metrics = layout_mtext(&mtext_opts);
            let block_w = metrics.line_widths.iter().cloned().fold(0.0_f32, f32::max) as f64;
            // Local up-axis position of the line that actually meets the leader.
            // BottomLine is an underline attachment, not a bottom-ink one: its
            // rule sits 0.2 × height below the selected line's real baseline.
            // Using that baseline also accounts for descenders, paragraph
            // spacing and exact line spacing; a nominal line-index calculation
            // leaves the rule off the leader on those labels.
            let attachment_up = underlined_line(vertical_attach, metrics.line_count)
                .and_then(|li| metrics.line_baselines.get(li).copied())
                .map(|baseline| baseline - height * MLEADER_UNDERLINE_GAP)
                .unwrap_or(0.0);

            // Horizontal leaders attach at the connection point: the dogleg runs
            // horizontally at connection_point.y, and the text's attachment line
            // (bottom/middle/top per text_left_attachment) sits there. Anchor the
            // insertion Y to that connection instead of the stored text_location.y
            // so the text meets the landing exactly — the two can disagree by a
            // few percent because our MText line height need not match the writer's.
            // For an underline attachment, `attachment_up` is the rule's offset
            // from the insertion, so the insertion goes on the opposite side of
            // the connection by that amount.
            //
            // Rotated text needs the same snap, but along the text frame's up
            // axis: an angled label is stored with its anchor free of the
            // connection (survey exporters leave it a block height plus a gap
            // away), and a Y-only snap would shear it off its rotated baseline.
            // Riding the up axis keeps the block's attachment line on the
            // connection's line.
            let (local_ins_x, local_ins_y) = match ml.text_attachment_direction {
                TextAttachmentDirectionType::Horizontal if rot.abs() < 1e-3 => (
                    local_ins_x,
                    ml.context
                        .leader_roots
                        .first()
                        .map(|r| r.connection_point.y as f32 - attachment_up)
                        .unwrap_or(ins.y as f32),
                ),
                TextAttachmentDirectionType::Horizontal => match ml.context.leader_roots.first() {
                    Some(root) => {
                        let up_x = -sin_r as f64;
                        let up_y = cos_r as f64;
                        let rel_x = root.connection_point.x - ins.x;
                        let rel_y = root.connection_point.y - ins.y;
                        let shift = rel_x * up_x + rel_y * up_y - attachment_up as f64;
                        (
                            (local_ins_x as f64 + up_x * shift) as f32,
                            (ins.y as f64 + up_y * shift) as f32,
                        )
                    }
                    _ => (local_ins_x, ins.y as f32),
                },
                _ => (local_ins_x, ins.y as f32),
            };

            // The same stored anchor can also sit stale ALONG the reading
            // direction: the block's approach edge belongs one landing gap past
            // the drawn landing's end, but writers often park it several line
            // heights down the row, letting the label pile onto its neighbour.
            // Only leaders whose landing actually advances along the reading
            // axis define that placement.
            let (local_ins_x, local_ins_y) = {
                let mut x = local_ins_x;
                let mut y = local_ins_y;
                if let Some(root) = ml.context.leader_roots.first() {
                    let dir_len = (root.direction.x * root.direction.x
                        + root.direction.y * root.direction.y)
                        .sqrt();
                    if dir_len > 1e-9 {
                        let along_x = cos_r as f64;
                        let along_y = sin_r as f64;
                        let d_along = (root.direction.x / dir_len) * along_x
                            + (root.direction.y / dir_len) * along_y;
                        if d_along.abs() > 0.5 {
                            let gap = ctx.landing_gap.max(0.0) * context_scale_correction as f64;
                            // Only a drawn landing (as `landing_endpoints`
                            // draws it) pushes the text out.
                            let landing = if ml.enable_landing && ml.enable_dogleg {
                                root.landing_distance.max(0.0) * context_scale_correction as f64
                            } else {
                                0.0
                            };
                            let target = d_along * (landing + gap);
                            // The edge that faces the leader: upstream when the
                            // landing advances with the reading direction,
                            // downstream when it runs against it.
                            let near_local = if d_along > 0.0 {
                                -block_w * h_anchor as f64
                            } else {
                                block_w * (1.0 - h_anchor as f64)
                            };
                            let current = (ins.x - root.connection_point.x) * along_x
                                + (ins.y - root.connection_point.y) * along_y
                                + near_local;
                            let shift = target - current;
                            x = (x as f64 + along_x * shift) as f32;
                            y = (y as f64 + along_y * shift) as f32;
                        }
                    }
                }
                (x, y)
            };

            // Final layout pass with the snapped anchor. Every inline format
            // code (`\f`, `\C`/`\c`, `\H`, `\W`, `\Q`, `\T`, `\A`, `\p…`,
            // decorations, stacked fractions, …) reaches the stroke output.
            // Stroke origins are already in offset-relative space because we
            // pass the snapped insertion.
            let layout = layout_mtext(&MTextRenderOpts {
                insertion: [local_ins_x as f64, local_ins_y as f64, z as f64],
                ..mtext_opts
            });
            let line_widths = &layout.line_widths;
            let max_line_w = line_widths.iter().cloned().fold(0.0_f32, f32::max);
            let line_h = layout.line_height;
            let v_offset = layout.v_offset;
            let n_lines = layout.line_count.max(1) as f32;

            // Resolve text color (falls back to entity color for ByLayer / ByBlock).
            let text_color = if selected {
                line_color
            } else {
                color_or_inherit(&ctx.text_color, entity_color)
            };

            {
                // SDF text: emit glyph quads instead of strokes (crisp at every
                // zoom, so no baseline/greek LOD). `layout_mtext` already placed
                // each run at its final position/rotation with a `GlyphRun`, so
                // this mirrors the top-level Text arm (anno = 1, origin as-is).
                // Base colour stays neutral — selection / hover recolouring is
                // done by the text-highlight overlay.
                let neutral = color_or_inherit(&ctx.text_color, entity_color);
                let mut sdf_verts: Vec<crate::display::text_vertex::TextVertex> = Vec::new();
                // Run-less stroke groups are decoration geometry (underline /
                // overline / strike-through, stacked-fraction bars) — they have
                // no glyphs to replace them, so they must be drawn as lines.
                // Web runs that require bidi or joined-script shaping keep their
                // already shaped vector geometry because the per-glyph SDF path has no
                // cluster-position data.
                let mut deco_pts: Vec<[f32; 3]> = Vec::new();
                let mut deco_fill: Vec<[f32; 3]> = Vec::new();
                let mut searchable: Vec<crate::display::wire_model::SearchableTextRun> =
                    Vec::new();
                if let Ok(mut atlas) = crate::text::sdf_atlas::text_atlas().lock() {
                    for ts in &layout.strokes {
                        let is_shaped = ts.run.as_ref().is_some_and(|r| {
                            crate::text::web_font::requires_shaping(&r.text)
                        });
                        if ts.run.is_none() || is_shaped {
                            for stroke in &ts.strokes {
                                if stroke.len() < 2 {
                                    continue;
                                }
                                if !deco_pts.is_empty() {
                                    deco_pts.push([f32::NAN; 3]);
                                }
                                for &[x, y] in stroke {
                                    deco_pts.push([
                                        ts.origin[0] as f32 + x,
                                        ts.origin[1] as f32 + y,
                                        z,
                                    ]);
                                }
                            }
                            for &[x, y] in &ts.fill_tris {
                                deco_fill.push([
                                    ts.origin[0] as f32 + x,
                                    ts.origin[1] as f32 + y,
                                    z,
                                ]);
                            }
                            continue;
                        }
                        let run = ts.run.as_ref().unwrap();
                        let (quads, pen_adv) = crate::text::glyph_quads::layout_glyph_quads(
                            &mut atlas,
                            run.height,
                            run.rotation,
                            run.width_factor,
                            run.oblique,
                            run.tracking,
                            &run.font,
                            run.bold,
                            &run.text,
                        );
                        // Inline `\C` / `\c` colour wins over the entity text
                        // colour, matching the top-level Text arm.
                        let gcolor = ts
                            .color
                            .map(|c| [c[0], c[1], c[2], neutral[3]])
                            .unwrap_or(neutral);
                        crate::scene::pipeline::text_gpu::push_glyph_vertices(
                            &mut sdf_verts,
                            &quads,
                            [ts.origin[0], ts.origin[1], z as f64],
                            1.0,
                            gcolor,
                            0.0,
                        );
                        {
                            use crate::display::wire_model::{
                                clean_searchable_text, SearchableTextRun,
                            };
                            let visible = clean_searchable_text(&run.text);
                            if !visible.is_empty() {
                                searchable.push(SearchableTextRun {
                                    text: visible,
                                    origin: [ts.origin[0], ts.origin[1], z as f64],
                                    height: run.height,
                                    rotation: run.rotation,
                                    color: gcolor,
                                    bold: run.bold,
                                    font: run.font.clone(),
                                    adv_width: pen_adv,
                                });
                            }
                        }
                    }
                }
                if !sdf_verts.is_empty() || !deco_pts.is_empty() || !deco_fill.is_empty() {
                    // Pick box from the glyph quads (f64 accumulate → f32).
                    let (mut nx, mut ny, mut xx, mut xy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for v in &sdf_verts {
                        let x = v.pos[0] as f64 + v.pos_low[0] as f64;
                        let y = v.pos[1] as f64 + v.pos_low[1] as f64;
                        nx = nx.min(x);
                        xx = xx.max(x);
                        ny = ny.min(y);
                        xy = xy.max(y);
                    }
                    for p in deco_pts.iter().filter(|p| p[0].is_finite()) {
                        nx = nx.min(p[0] as f64);
                        xx = xx.max(p[0] as f64);
                        ny = ny.min(p[1] as f64);
                        xy = xy.max(p[1] as f64);
                    }
                    for p in deco_fill.iter().filter(|p| p[0].is_finite()) {
                        nx = nx.min(p[0] as f64);
                        xx = xx.max(p[0] as f64);
                        ny = ny.min(p[1] as f64);
                        xy = xy.max(p[1] as f64);
                    }
                    wires.push(WireModel {
                        bg_adapt: None,
                        point_marker: None,
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        region_boundary_click: false,
                        reference_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        solid_normal: None,
                        render_instance: None,
                        stub: None,
                        dash_from_start: false,
                        element_linetype: None,
                        element_color: None,
                        dash_closure: Default::default(),
                        name: name.clone(),
                        effective_layer: None,
                        block_member: None,
                        color: neutral,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px,
                        aabb: [nx as f32, ny as f32, xx as f32, xy as f32],
                        plinegen: true,
                        geom: std::sync::Arc::new(crate::display::wire_model::WireGeom {
                            taper_widths: Vec::new(),
                            pattern_stations: Vec::new(),
                            text_verts: sdf_verts,
                            searchable_text: searchable,
                            points: deco_pts,
                            points_low: Default::default(),
                            snap_pts: vec![(
                                glam::DVec3::new(local_ins_x as f64, local_ins_y as f64, z as f64),
                                SnapHint::Node,
                            )],
                            tangent_geoms: vec![],
                            key_vertices: vec![],
                            surfaces: crate::display::wire_model::WireSurfaces {
                                fill_tris: deco_fill,
                                ..Default::default()
                            }
                            .into_optional(),
                            ..crate::display::wire_model::WireGeom::default()
                        }),
                    });
                }
            }

            // Underline attachments: the leader visually continues under the
            // text, so draw the landing on under the underlined line, from the
            // landing's end across the gap to the far end of the widest line,
            // in the leader's colour. When the writer stored no landing
            // distance at all (survey exporters park the leader at the text's
            // attachment row with none), the underline still has to reach the
            // connection itself, or the row of lines under the text stops a
            // gap short of the leader and never joins it.
            if let Some(li) = underlined_line(vertical_attach, layout.line_count) {
                let (mut x0, mut x1) = (-max_line_w * h_anchor, max_line_w * (1.0 - h_anchor));
                let mut connect_x = None;
                let mut connect_point = None;
                if let Some([x, y]) = landing_end_w {
                    connect_point = Some([x as f64, y as f64]);
                    connect_x = Some(
                        ((x as f64 - local_ins_x as f64) * cos_r as f64
                            + (y as f64 - local_ins_y as f64) * sin_r as f64)
                            as f32,
                    );
                } else if let Some(root) = ml.context.leader_roots.first() {
                    let cp = root.connection_point;
                    connect_point = Some([cp.x, cp.y]);
                    connect_x = Some(
                        ((cp.x - local_ins_x as f64) * cos_r as f64
                            + (cp.y - local_ins_y as f64) * sin_r as f64)
                            as f32,
                    );
                }
                if let Some(lx) = connect_x {
                    x0 = x0.min(lx);
                    x1 = x1.max(lx);
                }
                let y = layout
                    .line_baselines
                    .get(li)
                    .copied()
                    .unwrap_or(v_offset - li as f32 * line_h)
                    - height * MLEADER_UNDERLINE_GAP;
                // Build the joined rule around the leader endpoint itself.
                // Reconstructing that endpoint from a large f32 insertion plus
                // several local offsets leaves a visible gap at survey coords.
                let pts = if let (Some([jx, jy]), Some(lx)) = (connect_point, connect_x) {
                    let at = |x: f32| {
                        let along = x as f64 - lx as f64;
                        [
                            (jx + along * cos_r as f64) as f32,
                            (jy + along * sin_r as f64) as f32,
                            z,
                        ]
                    };
                    vec![at(x0), at(x1)]
                } else {
                    vec![
                        [
                            local_ins_x + x0 * cos_r - y * sin_r,
                            local_ins_y + x0 * sin_r + y * cos_r,
                            z,
                        ],
                        [
                            local_ins_x + x1 * cos_r - y * sin_r,
                            local_ins_y + x1 * sin_r + y * cos_r,
                            z,
                        ],
                    ]
                };
                wires.push(WireModel {
                    bg_adapt: None,
                    point_marker: None,
                    world_width: 0.0,
                    depth_override: None,
                    display_visible: true,
                    snap_only: false,
                    plot_visible: true,
                    fill_is_3d: false,
                    fill_is_2d_solid: false,
                    solid_normal: None,
                    render_instance: None,
                    dash_from_start: false,
                    name: name.clone(),
                    color: line_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    geom: std::sync::Arc::new(crate::display::wire_model::WireGeom {
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        text_verts: Vec::new(),
                        points: pts,
                        points_low: Default::default(),
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        ..crate::display::wire_model::WireGeom::default()
                    }),
                    ..Default::default()
                });
            }

            // Text frame / background-fill rectangle in local frame, then rotated to WCS.
            if ml.text_frame || ctx.background_mask_fill_on {
                // Border offset is a scale factor around the laid-out text box.
                let pad = height * ((ctx.background_scale_factor.max(1.0) as f32 - 1.0) * 0.5);
                // Box the glyphs that were actually laid out (valid for
                // vertical flow too); the metric-derived box is only the
                // no-glyph fallback.
                let lb = layout.local_bounds;
                let (block_left, block_bottom, block_right, block_top) = if lb[0] <= lb[2] {
                    (lb[0] - pad, lb[1] - pad, lb[2] + pad, lb[3] + pad)
                } else {
                    (
                        -max_line_w * h_anchor - pad,
                        v_offset - (n_lines - 1.0) * line_h - pad,
                        max_line_w * (1.0 - h_anchor) + pad,
                        v_offset + height + pad,
                    )
                };
                let local_corners: [[f32; 2]; 4] = [
                    [block_left, block_bottom],
                    [block_right, block_bottom],
                    [block_right, block_top],
                    [block_left, block_top],
                ];
                let wcs_corners: [[f32; 3]; 4] = std::array::from_fn(|i| {
                    let lx = local_corners[i][0];
                    let ly = local_corners[i][1];
                    let wx = local_ins_x + lx * cos_r - ly * sin_r;
                    let wy = local_ins_y + lx * sin_r + ly * cos_r;
                    [wx, wy, z]
                });

                // Background fill — emit two triangles; renders under the text strokes.
                // Code 292 (`background_mask_fill_on`) switches the fill on;
                // code 291 (`background_fill_enabled`) only picks the drawing
                // background over the stored fill colour.
                if ctx.background_mask_fill_on {
                    let mut fill_color = if selected {
                        line_color
                    } else if ctx.background_fill_enabled {
                        bg_color
                    } else {
                        color_or_inherit(&ctx.background_fill_color, entity_color)
                    };
                    fill_color[3] *= (1.0
                        - ctx.background_transparency.clamp(0, 90) as f32 / 100.0)
                        .clamp(0.1, 1.0);
                    let fill_tris: Vec<[f32; 3]> = vec![
                        wcs_corners[0],
                        wcs_corners[1],
                        wcs_corners[2],
                        wcs_corners[0],
                        wcs_corners[2],
                        wcs_corners[3],
                    ];
                    wires.push(WireModel {
                        bg_adapt: (!selected && ctx.background_fill_enabled).then(|| {
                            Box::new(crate::display::wire_model::BgAdaptInputs {
                                raw_color: fill_color,
                                canvas_color: true,
                                preserve_color: true,
                                ..Default::default()
                            })
                        }),
                        point_marker: None,
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        solid_normal: None,
                        render_instance: None,
                        dash_from_start: false,
                        name: name.clone(),
                        color: fill_color,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px: 1.0,
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        geom: std::sync::Arc::new(crate::display::wire_model::WireGeom {
                            taper_widths: Vec::new(),
                            pattern_stations: Vec::new(),
                            text_verts: Vec::new(),
                            points: vec![],
                            points_low: Default::default(),
                            snap_pts: vec![],
                            tangent_geoms: vec![],
                            key_vertices: vec![],
                            surfaces: crate::display::wire_model::WireSurfaces {
                                fill_tris,
                                ..Default::default()
                            }
                            .into_optional(),
                            ..crate::display::wire_model::WireGeom::default()
                        }),
                        ..Default::default()
                    });
                }

                // Text frame — closed rectangle, matches text color.
                if ml.text_frame {
                    let frame_points: Vec<[f32; 3]> = vec![
                        wcs_corners[0],
                        wcs_corners[1],
                        wcs_corners[2],
                        wcs_corners[3],
                        wcs_corners[0],
                    ];
                    wires.push(WireModel {
                        bg_adapt: None,
                        point_marker: None,
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        solid_normal: None,
                        render_instance: None,
                        dash_from_start: false,
                        name,
                        color: text_color,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px,
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        geom: std::sync::Arc::new(crate::display::wire_model::WireGeom {
                            taper_widths: Vec::new(),
                            pattern_stations: Vec::new(),
                            text_verts: Vec::new(),
                            points: frame_points,
                            points_low: Default::default(),
                            snap_pts: vec![],
                            tangent_geoms: vec![],
                            key_vertices: vec![],
                            ..crate::display::wire_model::WireGeom::default()
                        }),
                        ..Default::default()
                    });
                }
            }
        }

        (wires, world_wires)
    }
}
