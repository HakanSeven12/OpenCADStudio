// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::super::*;

/// Dim an already-bg-adapted colour toward the background when the entity's
/// layer is locked, so locked objects read as non-editable (they stay visible
/// and snappable). No-op for unlocked layers.
fn fade_if_locked(
    document: &acadrust::CadDocument,
    e: &EntityType,
    color: [f32; 4],
    bg: [f32; 4],
) -> [f32; 4] {
    if view::render::layer_locked(document, e) {
        crate::scene::cache::block_cache::fade_toward_bg(color, bg)
    } else {
        color
    }
}

/// The section mark's viewing direction (paper-space, unit), derived entirely
/// from the decoded model-documentation view graph:
///
/// 1. `symbol.view_rep_handle` → the parent view's `AcDbViewRep`.
/// 2. The parent's refs name the section view's `AcDbViewRep` (any ref that
///    owns an `AcDbViewRepSectionDefinition`), falling back to the document's
///    section views.
/// 3. Each view's refs include its `AcDbViewBorder` entity, which carries the
///    view's *active* viewport — the entity holding the real camera.
/// 4. The section camera's sight (−view_direction) is projected onto the parent
///    camera's right/up basis (same convention as `camera_from_view`), giving
///    the on-paper arrow direction.
///
/// `None` when any link is missing (DXF files, older DWGs) — the caller then
/// falls back to a geometric heuristic.
fn section_arrow_dir_from_views(
    document: &acadrust::CadDocument,
    s: &acadrust::entities::SectionSymbol,
) -> Option<[f64; 2]> {
    use acadrust::types::Handle as AHandle;
    if s.view_rep_handle.is_null() {
        return None;
    }
    // A view's active viewport, via its border entity among the ViewRep refs.
    let border_vp = |h: &AHandle| -> Option<AHandle> {
        match document.get_entity(*h)? {
            EntityType::ViewBorder(b) if !b.active_viewport.is_null() => Some(b.active_viewport),
            _ => None,
        }
    };
    let parent_vr = s.view_rep_handle;
    let parent_refs = document.view_rep_refs.get(&parent_vr)?;
    let parent_vp_h = parent_refs.iter().find_map(border_vp)?;
    let sect_vr = parent_refs
        .iter()
        .find(|h| **h != parent_vr && document.section_view_reps.contains(h))
        .or_else(|| document.section_view_reps.iter().find(|&&h| h != parent_vr))
        .copied()?;
    let sect_refs = document.view_rep_refs.get(&sect_vr)?;
    let sect_vp_h = sect_refs.iter().find_map(border_vp)?;

    let viewport = |h: &AHandle| {
        document.entities().find_map(|e| match e {
            EntityType::Viewport(v) if v.common.handle == *h => Some(v),
            _ => None,
        })
    };
    let pvp = viewport(&parent_vp_h)?;
    let svp = viewport(&sect_vp_h)?;

    // Section sight: view_direction points target→camera, sight is its inverse.
    let sight = -glam::DVec3::new(
        svp.view_direction.x,
        svp.view_direction.y,
        svp.view_direction.z,
    )
    .try_normalize()?;

    // Parent camera basis — the same yaw/pitch/roll convention the viewport
    // renderer uses (`camera_from_view`), so the arrow lands exactly where the
    // view content does.
    let vd = glam::Vec3::new(
        pvp.view_direction.x as f32,
        pvp.view_direction.y as f32,
        pvp.view_direction.z as f32,
    )
    .normalize_or(glam::Vec3::Z);
    let pitch = vd.z.clamp(-1.0, 1.0).asin();
    let yaw = if vd.x.abs() < 1e-6 && vd.y.abs() < 1e-6 {
        0.0_f32
    } else {
        vd.x.atan2(-vd.y)
    };
    let rotation = view::camera::yaw_pitch_to_quat(yaw, pitch, -(pvp.twist_angle as f32));
    let right = (rotation * glam::Vec3::X).as_dvec3();
    let up = (rotation * glam::Vec3::Y).as_dvec3();

    let d = glam::DVec2::new(sight.dot(right), sight.dot(up));
    let len = d.length();
    (len > 1e-6).then(|| [d.x / len, d.y / len])
}

/// Build the display wires for a decoded `AcDbSectionSymbol` (the "A-A" cut
/// mark on a Model-Documentation view), styled from its `AcDbSectionViewStyle`.
///
/// Everything drawn here is decoded from the file:
/// - Endpoints, end-tick lengths and the identifier come from the symbol.
/// - Whether the full cutting line is drawn (`show_plane_line` — off gives the
///   familiar broken line), the end segments (`show_end_lines`), the arrows
///   (`show_arrows`), and all sizes come from the section-view style's flags.
/// - The arrowhead shape resolves through the style's arrow block handle via
///   the shared dimension/leader arrow table (`arrow_from_block`); a null
///   handle is the standard ClosedFilled default.
/// - The arrow direction comes from the view graph
///   ([`section_arrow_dir_from_views`]); only when that chain is unavailable
///   does it fall back to the 90°-CCW rotation of END-A→END-B.
fn section_symbol_wires(
    document: &acadrust::CadDocument,
    s: &acadrust::entities::SectionSymbol,
    style: Option<&acadrust::entities::SectionViewStyle>,
    h: Handle,
    color: [f32; 4],
    sel: bool,
    lw_px: f32,
) -> Vec<WireModel> {
    use crate::scene::convert::tessellate::{append_arrow, arrow_from_block, ArrowKind, DimGeom};

    let nan = [f64::NAN; 3];
    let (ax, ay) = (s.end_a[0], s.end_a[1]);
    let (bx, by) = (s.end_b[0], s.end_b[1]);
    // Unit along the cut, pointing from END-B toward END-A (each end's tick
    // extends *outward*, away from the other end).
    let (dx, dy) = (ax - bx, ay - by);
    let clen = (dx * dx + dy * dy).sqrt().max(1e-9);
    let (ux, uy) = (dx / clen, dy / clen);
    // Viewing direction: from the decoded view graph when the chain resolves,
    // else the 90°-CCW rotation of (END-A → END-B).
    let [vx, vy] = section_arrow_dir_from_views(document, s).unwrap_or_else(|| {
        let (vx0, vy0) = (ay - by, bx - ax);
        let vlen = (vx0 * vx0 + vy0 * vy0).sqrt().max(1e-9);
        [vx0 / vlen, vy0 / vlen]
    });

    let (show_arrows, show_plane_line, show_end_lines, arrow_size, arrow_ext, label_h) = match style
    {
        Some(st) => (
            st.show_arrows,
            st.show_plane_line,
            st.show_end_lines,
            st.arrow_size,
            st.arrow_extension,
            st.label_height,
        ),
        None => {
            let t = s.tick_a.abs().max(s.tick_b.abs());
            (
                true,
                false,
                true,
                (t * 0.66).max(2.5),
                t.max(5.0),
                t.max(2.5),
            )
        }
    };
    // Arrowhead via the shared dimension/leader arrow table: the style's arrow
    // block handle (null → ClosedFilled), sized from the style.
    let arrow_kind = match style {
        Some(st) if st.arrow_start_handle != 0 => arrow_from_block(
            document,
            acadrust::types::Handle::from(st.arrow_start_handle),
            arrow_size as f32,
        ),
        _ => ArrowKind::Triangle {
            size: arrow_size as f32,
            filled: true,
            size_mul: 1.0,
        },
    };

    let mut lines: Vec<[f64; 3]> = Vec::new();
    let mut fill: Vec<[f64; 3]> = Vec::new();

    // Full cutting-plane line through the view, only when the style asks.
    if show_plane_line {
        lines.push([bx, by, 0.0]);
        lines.push([ax, ay, 0.0]);
    }

    // Each end: (endpoint, tick length, outward sign along the cut).
    for (ex, ey, tick, osign) in [
        (ax, ay, s.tick_a.abs(), 1.0),
        (bx, by, s.tick_b.abs(), -1.0),
    ] {
        let (ox, oy) = (ux * osign, uy * osign); // outward along the cut
        let tip = [ex + ox * tick, ey + oy * tick, 0.0]; // outer tick tip
                                                         // End segment: the short drawn extension past the end (the "broken"
                                                         // section line when show_plane_line is off).
        if show_end_lines && tick > 1e-9 {
            lines.push(nan);
            lines.push([ex, ey, 0.0]);
            lines.push(tip);
        }

        if show_arrows {
            // Arrowhead at the end of a short shaft along the viewing
            // direction, apex landing `arrow_ext` out from the tick tip. The
            // shared `append_arrow` takes the apex and the tip→base direction.
            let apex = [tip[0] + vx * arrow_ext, tip[1] + vy * arrow_ext, 0.0];
            let mut g = DimGeom::new();
            append_arrow(
                &mut g,
                glam::Vec3::new(apex[0] as f32, apex[1] as f32, 0.0),
                glam::Vec3::new(-vx as f32, -vy as f32, 0.0),
                &arrow_kind,
            );
            // Shaft from the tick tip to the arrowhead base.
            let a_len = match arrow_kind {
                ArrowKind::Triangle { size, size_mul, .. } => (size * size_mul) as f64,
                _ => arrow_size,
            };
            let base = [apex[0] - vx * a_len, apex[1] - vy * a_len, 0.0];
            lines.push(nan);
            lines.push(tip);
            lines.push(base);
            let mut it = g.dim_lines.chunks_exact(2);
            while let Some([p0, p1]) = it.next() {
                lines.push(nan);
                lines.push([p0[0] as f64, p0[1] as f64, 0.0]);
                lines.push([p1[0] as f64, p1[1] as f64, 0.0]);
            }
            for t in g.arrow_fill.chunks_exact(3) {
                for p in t {
                    fill.push([p[0] as f64, p[1] as f64, 0.0]);
                }
            }
        }

        // Identifier glyph outboard of the tick tip, centred on the cut line.
        if !s.label.trim().is_empty() && label_h > 1e-6 {
            let approx_w = label_h * 0.7 * s.label.chars().count().max(1) as f64;
            // Gap past the tip along the outward direction — the style's
            // `identifier_offset` when decoded; for the far (negative-outward)
            // end drop a text height so the glyph clears the tick. Centre the
            // run on the cut line's X.
            let gap = style
                .map(|st| st.label_offset)
                .filter(|v| *v > 1e-9)
                .unwrap_or(label_h * 0.35);
            let drop = if osign < 0.0 { label_h } else { 0.0 };
            let base = [ex - approx_w * 0.5, tip[1] + oy * gap - drop];
            let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
                [0.0, 0.0],
                label_h as f32,
                0.0,
                1.0,
                0.0,
                "standard",
                &s.label,
            );
            let mut tp: Vec<[f64; 3]> = Vec::new();
            for stroke in &strokes {
                if stroke.len() < 2 {
                    continue;
                }
                if !tp.is_empty() {
                    tp.push(nan);
                }
                for &[gx, gy] in stroke {
                    tp.push([base[0] + gx as f64, base[1] + gy as f64, 0.0]);
                }
            }
            lines.push(nan);
            lines.extend(tp);
        }
    }

    let mut wires = Vec::new();
    // Lines (ticks + shafts + glyphs): a fill-free wire so the strokes render.
    let (lp, lp_low) = convert::tessellate::points_to_ds(lines);
    let mut lw = WireModel::solid(h.value().to_string(), lp, color, sel);
    lw.points_low = lp_low;
    lw.line_weight_px = lw_px;
    wires.push(lw);
    // Filled arrowheads: a separate wire carrying only triangles.
    if !fill.is_empty() {
        let (fp, fp_low) = convert::tessellate::points_to_ds(fill);
        let mut fw = WireModel::solid(h.value().to_string(), Vec::new(), color, sel);
        fw.fill_tris = fp;
        fw.fill_tris_low = fp_low;
        fw.line_weight_px = lw_px;
        wires.push(fw);
    }
    wires
}

// ── Parallel tessellation free function ──────────────────────────────────────
//
// Takes only the `Send + Sync` data needed for tessellation so that
// `wires_for_block` can dispatch work across rayon's thread pool without
// requiring `Scene` (which contains `Rc<RefCell<...>>` and is `!Send`) to
// cross thread boundaries.

/// Tessellate a synthesised dimension-text entity through `tessellate_entity`
/// so it picks up the standard text LOD ladder (baseline / greek / full),
/// then re-color the returned wires with the dimension's resolved text colour
/// (so DIMCLRT / DIMSTYLE colours win over the synthetic Text's defaults).
pub(crate) fn tessellate_entity_dim_text(
    document: &acadrust::CadDocument,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    bg_color: [f32; 4],
    anno_scale: f32,
    e: &EntityType,
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    text_color: [f32; 4],
) -> Vec<WireModel> {
    let mut wires = tessellate_entity(
        document,
        selected,
        active_viewport,
        bg_color,
        anno_scale,
        None,
        e,
        None,
        view_aabb,
        world_per_pixel,
        false,
    );
    for w in &mut wires {
        // Synth dim text carries no real entity colour — paint everything
        // (including greek-LOD fill tris which read `wire.color`) with the
        // dim's text colour. Selection highlight already baked in by
        // tessellate_entity, so leave that alone.
        if !w.selected {
            w.color = text_color;
        }
    }
    wires
}

pub(crate) fn tessellate_entity(
    document: &acadrust::CadDocument,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    bg_color: [f32; 4],
    anno_scale: f32,
    annotation_scale_handle: Option<Handle>,
    e: &EntityType,
    block_cache: Option<&cache::block_cache::BlockCache>,
    // World-space XY view AABB (post `world_offset` subtraction). When
    // `Some`, entities whose AABB doesn't intersect this rect are skipped.
    view_aabb: Option<[f32; 4]>,
    // World units per screen pixel for LOD culling. `None` = no LOD.
    world_per_pixel: Option<f32>,
    // True only when tessellating content shown inside a paper-space viewport.
    // Retained through recursive INSERT expansion; PSLTSCALE itself is applied
    // by the viewport's GPU uniform so it never changes resident wire content.
    paper_space: bool,
) -> Vec<WireModel> {
    let contextual = crate::scene::annotative::entity_for_annotation_context(
        document,
        e,
        annotation_scale_handle,
    );
    let e = contextual.as_ref();
    let h = e.common().handle;
    let sel = selected.contains(&h);
    // Per-object annotation contexts store each representation relative to the
    // native/default scale. Resolve that ratio once before TEXT/MTEXT,
    // DIMENSION, and MULTILEADER reach their independent tessellation paths.
    let anno_scale = if matches!(
        e,
        EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::Dimension(_)
            | EntityType::MultiLeader(_)
    ) {
        crate::scene::annotative::effective_annotation_scale_for(
            document,
            e,
            anno_scale,
            annotation_scale_handle,
        )
    } else {
        anno_scale
    };

    // Frustum + LOD cull for non-Insert, non-Viewport entities. Insert is
    // handled separately (its WCS bbox depends on the block defn AABB ×
    // Insert transform — done inside expand_insert). Viewports always emit
    // so the viewport frame stays visible regardless of zoom.
    let needs_cull = view_aabb.is_some() || world_per_pixel.is_some();
    if needs_cull {
        match e {
            EntityType::Viewport(_) | EntityType::Insert(_) => {}
            _ => {
                let ab = entity_aabb(e);
                if ab != WireModel::UNBOUNDED_AABB {
                    if let Some(view) = view_aabb {
                        if cache::block_cache::aabb_disjoint_xy(ab, view) {
                            return vec![];
                        }
                    }
                    if let Some(wpp) = world_per_pixel {
                        let w_px = (ab[2] - ab[0]).abs();
                        let h_px = (ab[3] - ab[1]).abs();
                        // Keep in sync with `cache::block_cache::MIN_PIXEL_SIZE`.
                        // Text entities render as SDF glyph quads (crisp at every
                        // zoom, no LOD), so they must reach the full path even at
                        // sub-5 px — never substitute the stub.
                        let is_text = matches!(
                            e,
                            EntityType::Text(_)
                                | EntityType::MText(_)
                                | EntityType::AttributeDefinition(_)
                                | EntityType::AttributeEntity(_)
                                | EntityType::Tolerance(_)
                        );
                        // Face3D is exempt from the sub-pixel stub: it is trivially
                        // cheap to tessellate (4 corners → 2 tris), so there is no
                        // cost to draw it full at any zoom, and the cube-stub
                        // otherwise pops/coarsens flat faces across the threshold.
                        let is_face3d = matches!(e, EntityType::Face3D(_));
                        let is_3d_entity = matches!(
                            e,
                            EntityType::Solid3D(_)
                                | EntityType::Mesh(_)
                                | EntityType::PolyfaceMesh(_)
                                | EntityType::PolygonMesh(_)
                                | EntityType::Body(_)
                                | EntityType::Region(_)
                                | EntityType::Surface(_)
                        );
                        if !is_text && !is_face3d && w_px.max(h_px) / wpp < 5.0 {
                            // Sub-pixel entity: emit a stub instead of
                            // nothing so it stays visible / selectable /
                            // hit-test'able at any zoom. 2-D entities
                            // get the cheap diagonal segment; 3-D
                            // entities get an AABB cube so their
                            // footprint doesn't drift when the camera
                            // crosses the LOD threshold. See #19.
                            let (entity_color, _, _, _, aci_idx) =
                                view::render::render_style_for_viewport(document, e, active_viewport);
                            let entity_color = view::render::adapt_to_bg(entity_color, bg_color);
                            let entity_color = fade_if_locked(document, e, entity_color, bg_color);
                            if is_3d_entity {
                                // `ab` is already in the local frame
                                // (entity_aabb subtracted world_offset
                                // XY). The bbox z fields are still in
                                // WCS, so subtract `world_offset[2]` to
                                // match — otherwise the stub sits at a
                                // different z than the full tessellation
                                // and the geometry visibly shifts when
                                // the camera crosses the LOD threshold.
                                let bbox = e.as_entity().bounding_box();
                                let oz = 0.0_f64;
                                let z_min = (bbox.min.z - oz) as f32;
                                let z_max = (bbox.max.z - oz) as f32;
                                return vec![lod_stub_wire_3d(
                                    h.value().to_string(),
                                    entity_color,
                                    sel,
                                    aci_idx,
                                    ab,
                                    z_min,
                                    z_max,
                                )];
                            }
                            return vec![lod_stub_wire(
                                h.value().to_string(),
                                entity_color,
                                sel,
                                aci_idx,
                                ab,
                                0.0,
                                0.0,
                            )];
                        }
                    }
                }
            }
        }
    }

    if let EntityType::Viewport(vp) = e {
        // The sheet viewport (overall/id=1) is never shown — it represents the
        // paper boundary, not a user-defined content window.
        if !Scene::is_content_viewport(vp) {
            return vec![];
        }
        let is_active = active_viewport == Some(h);
        let is_locked = vp.status.locked;
        let color = if sel {
            [1.0, 1.0, 1.0, 1.0]
        } else if is_active {
            [1.0, 0.90, 0.20, 1.0]
        } else if is_locked {
            [0.90, 0.55, 0.10, 1.0]
        } else {
            [0.0, 0.75, 0.75, 1.0]
        };
        let (pattern_length, pattern) = if is_active {
            (1.5_f32, [0.8, -0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0_f32])
        } else {
            (0.0_f32, [0.0f32; 8])
        };
        let mut wires = convert::tessellate::tessellate(
            document,
            h,
            e,
            sel,
            color,
            pattern_length,
            pattern,
            1.5,
            1.0,
            annotation_scale_handle,
            world_per_pixel,
            bg_color,
            false,
        );
        let ab = entity_aabb(e);
        for w in &mut wires {
            set_wire_aabb(w, ab);
        }
        return wires;
    }

    let (entity_color, pattern_length, pattern, line_weight_px, aci) =
        view::render::render_style_for_viewport(document, e, active_viewport);
    let entity_color = view::render::adapt_to_bg(entity_color, bg_color);
    let entity_color = fade_if_locked(document, e, entity_color, bg_color);
    let lt_scale = document.header.linetype_scale as f32 * e.common().linetype_scale as f32;
    let lt_name = view::render::linetype_name_for_viewport(document, e, active_viewport);
    // Paper-space linetype scaling belongs to the viewport uniform. Keeping
    // resident geometry at its model-space scale prevents every MSPACE wheel
    // tick from rebuilding and uploading the viewport's complete wire set.
    let pslt_factor = 1.0_f32;
    // ── Proxy entity: draw its cached preview ───────────────────────────────
    //
    // An entity from an application we have no reader for (e.g. an Autodesk
    // Raster Design embedded raster image) arrives as `Unknown`. Its own data is
    // a private format we cannot decode — but it usually ships a proxy-graphics
    // blob, the vector preview its author cached for exactly this case. Draw it
    // when the object enabler is missing, so the entity
    // occupies its real place instead of silently disappearing.
    let proxy_blob: Option<std::borrow::Cow<'_, [u8]>> = match e {
        EntityType::Unknown(_) => e
            .common()
            .graphic_data
            .as_deref()
            .map(std::borrow::Cow::Borrowed),
        EntityType::Extended(entity) => match &entity.data {
            acadrust::entities::ExtendedEntityData::Proxy(proxy) => {
                Some(std::borrow::Cow::Owned(proxy.graphics.data()))
            }
            _ => None,
        },
        _ => None,
    };
    if let Some(blob) = proxy_blob {
            let dec = convert::proxy_graphics::decode(blob.as_ref());
            if !dec.polylines.is_empty() || !dec.texts.is_empty() {
                use crate::scene::convert::proxy_graphics::ProxyColor;
                use std::collections::BTreeMap;
                let nan = [f64::NAN; 3];
                // A specific ACI / RGB overrides the entity colour; ByLayer /
                // ByBlock inherit it.
                let resolve = |pc: ProxyColor| -> ([f32; 4], u8) {
                    match pc {
                        ProxyColor::Aci(a) => (
                            view::render::adapt_to_bg(
                                convert::tess_util::aci_to_rgba(&acadrust::types::Color::Index(a)),
                                bg_color,
                            ),
                            a,
                        ),
                        ProxyColor::Rgb(r, g, b) => (
                            view::render::adapt_to_bg(
                                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
                                bg_color,
                            ),
                            0,
                        ),
                        ProxyColor::Inherit => (entity_color, aci),
                    }
                };
                let mut wires = Vec::new();
                // Lines / shells: group by (colour, lineweight), one wire each.
                let mut groups: BTreeMap<(ProxyColor, i16), Vec<[f64; 3]>> = BTreeMap::new();
                for poly in &dec.polylines {
                    let buf = groups.entry((poly.color, poly.lineweight)).or_default();
                    if !buf.is_empty() {
                        buf.push(nan);
                    }
                    buf.extend_from_slice(&poly.points);
                }
                for ((pcolor, plw), pts64) in groups {
                    let (col, w_aci) = resolve(pcolor);
                    let lw_px = if plw >= 0 {
                        view::render::lineweight_to_px(&acadrust::types::LineWeight::Value(plw))
                    } else {
                        line_weight_px
                    };
                    let (pts, pts_low) = convert::tessellate::points_to_ds(pts64);
                    let mut w = WireModel::solid(h.value().to_string(), pts, col, sel);
                    w.points_low = pts_low;
                    w.line_weight_px = lw_px;
                    w.aci = w_aci;
                    wires.push(w);
                }
                // Text labels: draw the glyph strokes (simplex.shx etc. are
                // single-stroke fonts, so the outline is the character).
                for t in &dec.texts {
                    let font = t
                        .font
                        .trim()
                        .trim_end_matches(".shx")
                        .trim_end_matches(".SHX");
                    let font = if font.is_empty() { "standard" } else { font };
                    let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
                        [0.0, 0.0],
                        t.height as f32,
                        t.rotation as f32,
                        1.0,
                        0.0,
                        font,
                        &t.text,
                    );
                    let mut pts64: Vec<[f64; 3]> = Vec::new();
                    for stroke in &strokes {
                        if stroke.len() < 2 {
                            continue;
                        }
                        if !pts64.is_empty() {
                            pts64.push(nan);
                        }
                        for &[x, y] in stroke {
                            pts64.push([
                                t.position[0] + x as f64,
                                t.position[1] + y as f64,
                                t.position[2],
                            ]);
                        }
                    }
                    if pts64.len() >= 2 {
                        let (col, w_aci) = resolve(t.color);
                        let (pts, pts_low) = convert::tessellate::points_to_ds(pts64);
                        let mut w = WireModel::solid(h.value().to_string(), pts, col, sel);
                        w.points_low = pts_low;
                        w.line_weight_px = line_weight_px;
                        w.aci = w_aci;
                        wires.push(w);
                    }
                }
                if !wires.is_empty() {
                    return wires;
                }
            }
    }

    // ── Section symbol (AcDbSectionSymbol): draw the "A-A" cut mark ──────────
    //
    // The DWG reader decodes the two cut-line endpoints, the signed end ticks
    // and the identifier. Synthesize the end ticks + arrowheads + label glyphs
    // so the mark is visible on the layout. The raw
    // record is still preserved for lossless write-back.
    if let EntityType::SectionSymbol(s) = e {
        return section_symbol_wires(
            document,
            s,
            document.section_view_style.as_ref(),
            h,
            entity_color,
            sel,
            line_weight_px,
        );
    }
    // Drawing-view borders are non-plotting aids: nothing is drawn normally,
    // but the view must still be click-selectable anywhere inside its
    // rectangle — emit an invisible pick-only wire carrying the border rect as
    // its interior pick surface. When selected, the rect outline is drawn so
    // the selection has visible feedback.
    if let EntityType::ViewBorder(b) = e {
        let (x0, y0) = (b.min[0], b.min[1]);
        let (x1, y1) = (b.max[0], b.max[1]);
        if !(x1 > x0 && y1 > y0) {
            return vec![];
        }
        let (pick_tris, pick_tris_low) =
            super::tessellate::points_to_ds(crate::entities::common::quad_pick_tris(&[
                [x0, y0, 0.0],
                [x1, y0, 0.0],
                [x1, y1, 0.0],
                [x0, y1, 0.0],
            ]));
        let points: Vec<[f32; 3]> = if sel {
            vec![
                [x0 as f32, y0 as f32, 0.0],
                [x1 as f32, y0 as f32, 0.0],
                [x1 as f32, y1 as f32, 0.0],
                [x0 as f32, y1 as f32, 0.0],
                [x0 as f32, y0 as f32, 0.0],
            ]
        } else {
            Vec::new()
        };
        return vec![WireModel {
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            pick_tris,
            pick_tris_low,
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: h.value().to_string(),
            points,
            points_low: Vec::new(),
            color: WireModel::SELECTED,
            selected: sel,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![[x0, y0, 0.0], [x1, y1, 0.0]],
            aabb: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        }];
    }

    if let EntityType::Dimension(dim) = e {
        let aabb = entity_aabb(e);
        use crate::entities::dimension::DimensionTess;
        let mut wires = dim.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            anno_scale,
            selected,
            active_viewport,
            bg_color,
            view_aabb,
            world_per_pixel,
        );
        for w in &mut wires {
            w.aci = aci;
            // The whole-dimension box is a broad-phase hint for stroke/fill
            // wires (picked by proximity). An empty SDF-text wire instead
            // keeps its own tight glyph-box AABB so the text pick box hugs the
            // text — clicking empty space inside the dimension selects nothing,
            // only the lines or the text do.
            if !w.points.is_empty() || !w.fill_tris.is_empty() {
                set_wire_aabb(w, aabb);
            }
        }
        return wires;
    }

    if let EntityType::MultiLeader(ml) = e {
        let aabb = entity_aabb(e);
        use crate::entities::multileader::MultiLeaderTess;
        let mut wires = ml.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            anno_scale,
            world_per_pixel,
            bg_color,
        );
        for w in &mut wires {
            w.aci = aci;
            // As with dimensions: keep the whole-leader box only on stroke/fill
            // wires; empty SDF-text wires keep their tight glyph-box AABB.
            if !w.points.is_empty() || !w.fill_tris.is_empty() {
                set_wire_aabb(w, aabb);
            }
        }
        return wires;
    }

    // ── Table baked-block fast path ─────────────────────────────────────────
    //
    // A table may store final rendered geometry (cell text, gridlines,
    // fill) into a per-instance block (usually `*T###`) referenced through
    // `table.block_record_handle`. The block's text uses the *displayed*
    // height; synthesising cells from `self.rows + TableStyle` instead would
    // re-apply the table's scale factor on top of already-baked geometry.
    // When the block exists we render it directly. Same pattern as
    // Dimension's `block_name`.
    if let EntityType::Table(table) = e {
        if let Some(record) = table.block_record_handle.and_then(|handle| {
            document
                .block_records
                .iter()
                .find(|record| record.handle == handle)
        }) {
            if !record.entity_handles.is_empty() {
                let mut insert =
                    acadrust::entities::Insert::new(
                        record.name.clone(),
                        table.insertion_point,
                    );
                insert.rotation = table
                    .horizontal_direction
                    .y
                    .atan2(table.horizontal_direction.x);
                insert.common = table.common.clone();

                let depths = rustc_hash::FxHashMap::default();
                let graph = crate::scene::render_graph::RenderSceneGraph::new(
                    document,
                    None,
                    annotation_scale_handle,
                    true,
                    &depths,
                );
                let mut wires = Vec::new();
                graph.walk_insert(
                    &insert,
                    h,
                    |_, _| true,
                    |sub, context| {
                        let has_book_color =
                            view::render::has_resolved_book_color(document, sub);
                        let color_byblock = !has_book_color
                            && sub.common().color
                                == acadrust::types::Color::ByBlock;
                        let color_layer0 = !has_book_color
                            && view::render::is_effective_layer_zero(
                                &sub.common().layer,
                            )
                            && sub.common().color
                                == acadrust::types::Color::ByLayer;
                        let style = context.style_for(document, sub);
                        let mut placed = sub.clone();
                        placed.apply_transform(&context.transform);
                        let sub_wires = tessellate_entity(
                            document,
                            selected,
                            active_viewport,
                            bg_color,
                            anno_scale,
                            annotation_scale_handle,
                            &placed,
                            block_cache,
                            view_aabb,
                            world_per_pixel,
                            paper_space,
                        );
                        for mut wire in sub_wires {
                            wire.name = h.value().to_string();
                            if color_byblock {
                                wire.color = if sel {
                                    WireModel::SELECTED
                                } else {
                                    view::render::adapt_to_bg(style.0, bg_color)
                                };
                                wire.aci = style.4;
                            } else if color_layer0 && !sel {
                                wire.color =
                                    view::render::adapt_to_bg(style.0, bg_color);
                                wire.aci = style.4;
                            }
                            wires.push(wire);
                        }
                    },
                );
                if !wires.is_empty() {
                    let aabb = entity_aabb(e);
                    for wire in &mut wires {
                        if !wire.points.is_empty() || !wire.fill_tris.is_empty() {
                            set_wire_aabb(wire, aabb);
                        }
                    }
                    return wires;
                }
            }
        }

        let table_anno = 1.0;
        let mut wires = crate::entities::table::tessellate_table(
            table,
            document,
            sel,
            entity_color,
            line_weight_px,
            table_anno,
        );
        for insert in
            crate::entities::table::block_cell_inserts(table, document, table_anno)
        {
            wires.extend(tessellate_entity(
                document,
                selected,
                active_viewport,
                bg_color,
                1.0,
                None,
                &EntityType::Insert(insert),
                block_cache,
                view_aabb,
                world_per_pixel,
                paper_space,
            ));
        }
        if !wires.is_empty() {
            let aabb = entity_aabb(e);
            for wire in &mut wires {
                wire.aci = aci;
                if !wire.points.is_empty() || !wire.fill_tris.is_empty() {
                    set_wire_aabb(wire, aabb);
                }
            }
            return wires;
        }
    }

    if let EntityType::Insert(ins) = e {
        // Resolve the INSERT's own style so ByBlock sub-entities can inherit it.
        let (ins_color, ins_pat_len, ins_pat, ins_lw_px, _) =
            view::render::render_style_for_viewport(document, e, active_viewport);
        let ins_color = view::render::adapt_to_bg(ins_color, bg_color);
        // Resolve the INSERT's *layer* style — the layer-0 inheritance target
        // for sub-entities on layer "0" with ByLayer properties (#221).
        let ins_layer = {
            let mut s = view::render::layer_render_style_viewport(
                document,
                &ins.common.layer,
                active_viewport,
            );
            s.color = view::render::adapt_to_bg(s.color, bg_color);
            s
        };
        let ip = glam::Vec3::new(
            (ins.insert_point.x) as f32,
            (ins.insert_point.y) as f32,
            (ins.insert_point.z) as f32,
        );
        let marker = WireModel {
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: h.value().to_string(),
            points: vec![],
            points_low: Vec::new(),
            color: entity_color,
            selected: sel,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![(ip.as_dvec3(), model::wire_model::SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        };

        let fallback_depths = rustc_hash::FxHashMap::default();
        let fallback_cache;
        let cache = match block_cache {
            Some(cache) if cache.defn(&ins.block_name).is_some() => cache,
            _ => {
                fallback_cache = cache::block_cache::BlockCache::build_for_block(
                    document,
                    &ins.block_name,
                    anno_scale,
                    annotation_scale_handle,
                    true,
                    bg_color,
                    active_viewport,
                    &fallback_depths,
                );
                &fallback_cache
            }
        };
        let is_xref = document
            .block_records
            .get(&ins.block_name)
            .map(|record| record.flags.is_xref || record.flags.is_xref_overlay)
            .unwrap_or(false);
        let mut wires = cache::block_cache::expand_insert(
            cache,
            ins,
            h,
            ins_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            ins_layer,
            sel,
            pslt_factor,
            view_aabb,
            world_per_pixel,
            is_xref,
            bg_color,
            anno_scale,
        )
        .unwrap_or_default();

        if let Some(filter) = pick::xclip::insert_spatial_filter(document, ins) {
            let transform = crate::scene::render_graph::insert_transform(document, ins);
            let polygon =
                pick::xclip::world_clip_polygon_for_transform(filter, &transform);
            pick::xclip::clip_wires(&mut wires, &polygon);
            if document.header.xclip_frame != 0 && polygon.len() >= 3 {
                wires.push(pick::xclip::frame_wire(
                    &polygon,
                    format!("{}_xclipframe", h.value()),
                    ins_color,
                    sel,
                    ins_lw_px,
                ));
            }
        }

        crate::entities::insert::append_insert_attribute_wires(
            &mut wires,
            document,
            ins,
            h,
            sel,
            ins_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            ins_layer,
            bg_color,
            is_xref,
            pslt_factor,
            anno_scale,
        );
        wires.push(marker);
        return wires;
    }

    let aabb = entity_aabb(e);

    // TEXT / MTEXT / ATTDEF / ATTRIB / Tolerance all render as SDF glyph quads
    // (crisp at every zoom), so there is no text LOD ladder — they fall through
    // to the full tessellation path below.

    let mut bases = convert::tessellate::tessellate(
        document,
        h,
        e,
        sel,
        entity_color,
        pattern_length,
        pattern,
        line_weight_px,
        anno_scale,
        annotation_scale_handle,
        world_per_pixel,
        bg_color,
        false,
    );
    for b in &mut bases {
        b.aci = aci;
        // SDF text wires carry a glyph-bounds AABB (the true text extent) set
        // in the Text arm; entity_aabb would mis-place the pick box for MTEXT,
        // so don't clobber it. Every other wire takes the entity AABB.
        if b.text_verts.is_empty() {
            set_wire_aabb(b, aabb);
        }
    }

    // Complex linetypes (with embedded shapes / text) expand the *base*
    // polyline along its tangent. Text-type entities never have a complex
    // linetype assigned, so we only consult the first wire here — multi-wire
    // returns come exclusively from MTEXT colour splits which can't trigger
    // this path.
    if let Some(clt) = crate::io::linetypes::resolve_complex_lt(document, lt_name) {
        if let Some(base) = bases.first() {
            // Walk the dash / glyph layout in a local frame so `apply_along`'s
            // f32 math stays precise, then lift each wire back to world DS.
            // `base.points` is the double-single HIGH half; at UTM coordinates
            // (−1.2M) that alone quantises to ~0.1, which jitters the complex
            // linetype's shapes and dashes. Reconstruct the absolute f64 path
            // (high + low) and re-origin it at the first vertex before walking.
            let abs = |i: usize| -> [f64; 3] {
                let p = base.points[i];
                let l = base.points_low.get(i).copied().unwrap_or([0.0; 3]);
                [
                    p[0] as f64 + l[0] as f64,
                    p[1] as f64 + l[1] as f64,
                    p[2] as f64 + l[2] as f64,
                ]
            };
            let origin = base
                .points
                .iter()
                .position(|p| !p[0].is_nan())
                .map(&abs)
                .unwrap_or([0.0; 3]);
            let local: Vec<[f32; 3]> = base
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if p[0].is_nan() {
                        [f32::NAN; 3]
                    } else {
                        let a = abs(i);
                        [
                            (a[0] - origin[0]) as f32,
                            (a[1] - origin[1]) as f32,
                            (a[2] - origin[2]) as f32,
                        ]
                    }
                })
                .collect();
            let mut wires = text::complex_lt::apply_along(
                &base.name,
                &local,
                &clt,
                (lt_scale * pslt_factor).max(1e-4),
                entity_color,
                sel,
                base.line_weight_px,
                // Single (non-MLINE) entity: keep the from-start tiling (no shared
                // A-type reference — there are no sibling elements to align with).
                None,
            );
            if !wires.is_empty() {
                for w in &mut wires {
                    convert::tessellate::shift_wire_to_world(w, origin);
                    set_wire_aabb(w, aabb);
                }
                return wires;
            }
        }
    }

    // DGN line-style: the linetype's real pattern lives in DGN line-style objects
    // (empty standard LTYPE), so `resolve_complex_lt` sees nothing. Render its
    // symbol blocks (e.g. a pipe's end circles) at the polyline endpoints and
    // apply the typed DGN stroke pattern to its parallel walls.
    let dgn_syms = convert::dgn_linestyle::symbol_blocks(document, lt_name);
    if !dgn_syms.is_empty() {
        let verts = convert::dgn_linestyle::polyline_points(e);
        if verts.len() >= 2 {
            let display_scale = lt_scale.max(1.0e-4) as f64;
            // The pipe body is drawn as two parallel walls, not a single centre
            // line: offset the host polyline by ±(symbol radius) so each wall
            // sits tangent to the end circles, and replace the centre line with
            // them. The radius is the rendered symbol extent after applying the
            // entity/global linetype display scale.
            let radius = dgn_syms
                .iter()
                .map(|s| {
                    convert::dgn_linestyle::symbol_radius(
                        document,
                        s.block,
                        s.scale / display_scale,
                    )
                })
                .fold(0.0_f64, f64::max);
            if radius > 1e-6 {
                // Preserve the typed stroke's signed dash/gap sequence and scale
                // every element by the linetype display scale. Combined with the
                // `dash_from_start` flag below, each wall tiles from its own start
                // vertex with no A-type end alignment.
                let native = convert::dgn_linestyle::wall_dashes(document, lt_name);
                let (wall_pat, wall_pat_len) = if !native.is_empty() {
                    let mut pat = [0.0_f32; 8];
                    let mut length = 0.0_f32;
                    for (slot, value) in pat.iter_mut().zip(native.iter().take(8)) {
                        let scaled = (*value * display_scale) as f32;
                        if scaled.is_finite() && scaled.abs() > 1.0e-6 {
                            *slot = scaled;
                            length += scaled.abs();
                        }
                    }
                    (pat, length)
                } else {
                    ([0.0_f32; 8], 0.0)
                };
                let mut rails = Vec::new();
                for sgn in [1.0_f64, -1.0] {
                    if let Some(off) = convert::dgn_linestyle::offset_host_entity(e, sgn * radius) {
                        let mut w = convert::tessellate::tessellate(
                            document,
                            h,
                            &off,
                            sel,
                            entity_color,
                            pattern_length,
                            pattern,
                            line_weight_px,
                            anno_scale,
                            annotation_scale_handle,
                            world_per_pixel,
                            bg_color,
                            false,
                        );
                        for x in &mut w {
                            x.aci = aci;
                            set_wire_aabb(x, aabb);
                            if wall_pat_len > 1e-6 {
                                x.pattern = wall_pat;
                                x.pattern_length = wall_pat_len;
                                // DGN: draw the dash from this wall's own start
                                // vertex, no A-type end forcing. Each wall is a
                                // separate wire, so the two tile independently.
                                x.dash_from_start = true;
                            }
                        }
                        rails.append(&mut w);
                    }
                }
                if !rails.is_empty() {
                    bases = rails;
                }
            }
            let last = *verts.last().unwrap();
            for (i, sym) in dgn_syms.iter().enumerate() {
                let at = if i == 0 { verts[0] } else { last };
                let mut wires = convert::dgn_linestyle::place_block_wires(
                    document,
                    sym.block,
                    sym.scale / display_scale,
                    at,
                    entity_color,
                    line_weight_px,
                    anno_scale,
                    world_per_pixel,
                    bg_color,
                );
                // The symbol wires come back named after the anonymous block's
                // internal entity handle. Re-key them to the host entity (like the
                // walls above) so the whole DET pipe picks/selects as one entity
                // instead of the symbols acting as separate phantom entities.
                let host_name = h.value().to_string();
                for w in &mut wires {
                    w.name = host_name.clone();
                    w.aci = aci;
                    set_wire_aabb(w, aabb);
                    // A wipeout's frame sits ABOVE its own mask: the mask
                    // draws at the entity's rank, the frame half a rank
                    // closer, so the later mask pass fails the depth test at
                    // the frame's pixels instead of erasing it.
                    if matches!(e, EntityType::Wipeout(_)) {
                        w.depth_override = Some(0.5);
                    }
                }
                bases.extend(wires);
            }
        }
    }

    bases
}

/// Build the 4 OBB corners (CCW: bl, br, tr, tl) of a Text / MText entity
/// in its **native frame** — for top-level entities this is world coords,
/// for block-defn subs it's block-local. No offset/transform applied.
/// Width is approximated from glyph height × character count (TEXT) or
/// from `rectangle_width` (MTEXT). Returns `None` for non-text entities.
///
/// `mtext_lines_override` lets the caller plug in a wrap-aware line count
/// (from `text_support::mtext_line_count`). Without it, MText's OBB
/// height collapses to a single line when the file omits `rectangle_height`,
/// which makes downstream per-line LOD math degenerate.

/// Build a "low-LOD stub" wire for an entity that would otherwise be culled
/// to nothing — the entity's AABB diagonal as a 2-point segment, plus the
/// AABB itself so window / crossing selection picks the entity up. The
/// stored `selected` flag tracks across zoom levels so highlight visuals
/// don't disappear when the LOD level changes. See #19.
fn lod_stub_wire(
    name: String,
    color: [f32; 4],
    selected: bool,
    aci: u8,
    aabb: [f32; 4],
    z_min: f32,
    z_max: f32,
) -> WireModel {
    let [ax, ay, bx, by] = aabb;
    let cx = (ax + bx) * 0.5;
    let cy = (ay + by) * 0.5;
    let cz = (z_min + z_max) * 0.5;
    // Mirror what tessellate.rs does for the non-stub paths: bake the
    // selection-highlight colour into the wire so a re-tessellate triggered
    // by a zoom-induced LOD change keeps the entity highlighted. Without
    // this swap the wire's `selected` flag is true but its colour stays at
    // the entity's own hue, so the user sees the highlight vanish at the
    // LOD boundary. #19.
    let stored_color = if selected { WireModel::SELECTED } else { color };
    WireModel {
        taper_widths: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name,
        // Diagonal of the entity's 3D AABB so depth tests against
        // shaded / hidden-line geometry are correct — the stub doesn't
        // flatten to z=0 and pop in front of objects that sit at a
        // different elevation. 2D entities (text fallbacks) pass
        // z_min = z_max = 0 to keep the historical behaviour.
        points: vec![[ax, ay, z_min], [bx, by, z_max]],
        points_low: Vec::new(),
        color: stored_color,
        selected,
        aci,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: vec![[cx as f64, cy as f64, cz as f64]],
        aabb,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Sub-pixel LOD stub for 3D entities. Emits the entity's 3D AABB as a
/// 12-edge cube so the geometry occupies the same screen footprint and
/// depth range as the full tessellation, just with a tiny constant cost
/// (12 line segments). Without this, the diagonal stub used by
/// `lod_stub_wire` cuts off at two opposite bbox corners and drifts
/// visibly when the camera crosses the LOD threshold.
fn lod_stub_wire_3d(
    name: String,
    color: [f32; 4],
    selected: bool,
    aci: u8,
    aabb: [f32; 4],
    z_min: f32,
    z_max: f32,
) -> WireModel {
    let [x0, y0, x1, y1] = aabb;
    let (z0, z1) = if z_min <= z_max {
        (z_min, z_max)
    } else {
        (z_max, z_min)
    };
    let p = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ];
    // 12 edges = 4 bottom-face + 4 top-face + 4 vertical connectors.
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let mut points: Vec<[f32; 3]> = Vec::with_capacity(EDGES.len() * 3);
    for (a, b) in EDGES {
        if !points.is_empty() {
            points.push([f32::NAN; 3]);
        }
        points.push(p[a]);
        points.push(p[b]);
    }
    let stored_color = if selected { WireModel::SELECTED } else { color };
    WireModel {
        taper_widths: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name,
        points,
        points_low: Vec::new(),
        color: stored_color,
        selected,
        aci,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        // No `key_vertices` — Face3DGpu requires 4 corners to emit a
        // fill quad, and we don't want this stub painted as a solid
        // face. The wire pass still draws its 12 edges.
        key_vertices: vec![],
        aabb,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Tessellate each visible AttributeEntity attached to an Insert and append
/// the resulting wires. AttributeEntity positions are already in WCS — the
/// INSERT only stamps the geometry once, attribute text sits at the world
/// position recorded on each ATTRIB. See #20.
#[allow(clippy::too_many_arguments)]
/// World-space XY AABB of a wire computed from its own points (double-single
/// `points + points_low` sum = absolute world, matching `wire_in_range`'s
/// `cursor_world`). Used as a fallback when an entity's `bounding_box()` is
/// degenerate/unimplemented (`entity_aabb` → `UNBOUNDED`), so the wire still
/// gets a real, cullable box instead of never being pre-rejected during snap.
pub(crate) fn wire_points_aabb(w: &WireModel) -> [f32; 4] {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    let mut any = false;
    for (i, p) in w.points.iter().enumerate() {
        let lo = w.points_low.get(i).copied().unwrap_or([0.0; 3]);
        let (x, y) = (p[0] + lo[0], p[1] + lo[1]);
        if x.is_finite() && y.is_finite() {
            min[0] = min[0].min(x);
            min[1] = min[1].min(y);
            max[0] = max[0].max(x);
            max[1] = max[1].max(y);
            any = true;
        }
    }
    if any {
        [min[0], min[1], max[0], max[1]]
    } else {
        WireModel::UNBOUNDED_AABB
    }
}

/// Assign `entity_box` as `w`'s cullable box, widened to cover the wire's
/// actual tessellated geometry and pick-only geometry.
///
/// `entity_aabb`'s box comes from acadrust's `bounding_box()`, which for a
/// polyline is often the box of its stored vertices — it may know nothing about
/// bulge arcs between them, the band a width paints around them, or the wall a
/// thickness extrudes. Hit-testing rejects on this box before it looks at the
/// wire segments or `pick_tris`, so a box that stops short of the drawn geometry
/// makes that geometry silently unpickable.
pub(crate) fn set_wire_aabb(w: &mut WireModel, entity_box: [f32; 4]) {
    let mut out = if entity_box == WireModel::UNBOUNDED_AABB {
        wire_points_aabb(w)
    } else {
        entity_box
    };

    let mut extend = |p: [f32; 3], lo: [f32; 3]| {
        let (x, y) = (p[0] + lo[0], p[1] + lo[1]);
        if x.is_finite() && y.is_finite() {
            if out == WireModel::UNBOUNDED_AABB {
                out = [x, y, x, y];
            } else {
                out[0] = out[0].min(x);
                out[1] = out[1].min(y);
                out[2] = out[2].max(x);
                out[3] = out[3].max(y);
            }
        }
    };

    for (i, &p) in w.points.iter().enumerate() {
        extend(p, w.points_low.get(i).copied().unwrap_or([0.0; 3]));
    }
    for (i, &p) in w.pick_tris.iter().enumerate() {
        extend(p, w.pick_tris_low.get(i).copied().unwrap_or([0.0; 3]));
    }

    w.aabb = out;
}

pub(crate) fn entity_aabb(e: &acadrust::EntityType) -> [f32; 4] {
    let bbox = e.as_entity().bounding_box();
    let min_x = (bbox.min.x) as f32;
    let min_y = (bbox.min.y) as f32;
    let max_x = (bbox.max.x) as f32;
    let max_y = (bbox.max.y) as f32;
    // The all-zero box is bounding_box()'s Default — returned by entities with
    // no usable box (unimplemented) — so treat it as UNBOUNDED (never
    // pre-rejected). A genuinely zero-size box *away* from the origin (e.g. a
    // POINT) is a valid, cullable position: keep it, otherwise point-heavy
    // drawings fill the always-checked set and stall hit-testing.
    if min_x == 0.0 && min_y == 0.0 && max_x == 0.0 && max_y == 0.0 {
        return WireModel::UNBOUNDED_AABB;
    }
    [min_x, min_y, max_x, max_y]
}

/// AABB of `e` in WCS f64 (no world_offset subtraction). `None` for
/// entities whose `bounding_box()` returned the degenerate default
/// (which `entity_aabb` collapses to `UNBOUNDED_AABB`). Quadtree
/// indexing uses this so changing `world_offset` doesn't invalidate
/// the index.
pub(crate) fn entity_world_aabb_f64(e: &acadrust::EntityType) -> Option<[f64; 4]> {
    let bbox = e.as_entity().bounding_box();
    let (xmin, ymin, xmax, ymax) = (bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y);
    if xmin == xmax && ymin == ymax {
        return None;
    }
    if !xmin.is_finite() || !ymin.is_finite() || !xmax.is_finite() || !ymax.is_finite() {
        return None;
    }
    Some([xmin, ymin, xmax, ymax])
}

/// True if `e` is a type the quadtree should skip. `Insert` and
/// `Viewport` are sized only after extra transformation; tessellation
/// already handles them via dedicated code paths. `Block`/`BlockEnd`
/// are block-defn sentinels with no geometry.
pub(crate) fn is_unindexable_entity(e: &acadrust::EntityType) -> bool {
    use acadrust::EntityType as E;
    matches!(
        e,
        E::Insert(_) | E::Viewport(_) | E::Block(_) | E::BlockEnd(_)
    )
}
