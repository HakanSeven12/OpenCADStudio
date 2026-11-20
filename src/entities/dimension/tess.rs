// Dimension tessellation.

use super::*;

pub trait DimensionTess {
    #[allow(clippy::too_many_arguments)]
    fn tessellate(
        &self,
        document: &Document,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<document::Handle>,
        active_viewport: Option<document::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        reference_only: &rustc_hash::FxHashSet<document::Handle>,
    ) -> Vec<WireModel>;
}

impl DimensionTess for Dimension {
    #[allow(clippy::too_many_arguments)]
    fn tessellate(
        &self,
        document: &Document,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<document::Handle>,
        active_viewport: Option<document::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        reference_only: &rustc_hash::FxHashSet<document::Handle>,
    ) -> Vec<WireModel> {
        tessellate_dimension_inner(
            document,
            handle,
            self,
            selected,
            entity_color,
            line_weight_px,
            anno_scale,
            selected_set,
            active_viewport,
            bg_color,
            view_aabb,
            world_per_pixel,
            reference_only,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_dimension_inner(
    document: &Document,
    handle: Handle,
    dim: &Dimension,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    anno_scale: f32,
    // LOD hints — when present, synthesised dim text routes through the
    // top-level LOD ladder (baseline / greek / full) instead of the render
    // path so far-out drawings collapse to a colored rect or baseline.
    selected_set: &rustc_hash::FxHashSet<document::Handle>,
    active_viewport: Option<document::Handle>,
    bg_color: [f32; 4],
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    reference_only: &rustc_hash::FxHashSet<document::Handle>,
) -> Vec<WireModel> {
    if let Dimension::Arc(arc) = dim {
        if let Some((local_arc, normal)) = arc_dimension_in_ocs(arc) {
            let mut wires = tessellate_dimension_inner(
                document,
                handle,
                &Dimension::Arc(local_arc),
                selected,
                entity_color,
                line_weight_px,
                anno_scale,
                selected_set,
                active_viewport,
                bg_color,
                None,
                world_per_pixel,
                reference_only,
            );
            for wire in &mut wires {
                map_wire_ocs_to_wcs(wire, normal);
            }
            return wires;
        }
    }
    if let Dimension::LargeRadial(radial) = dim {
        if let Some((local_radial, normal)) = large_radial_dimension_in_ocs(radial) {
            let mut wires = tessellate_dimension_inner(
                document,
                handle,
                &Dimension::LargeRadial(local_radial),
                selected,
                entity_color,
                line_weight_px,
                anno_scale,
                selected_set,
                active_viewport,
                bg_color,
                None,
                world_per_pixel,
                reference_only,
            );
            for wire in &mut wires {
                map_wire_ocs_to_wcs(wire, normal);
            }
            return wires;
        }
    }
    if let Some((local, normal)) = linear_dimension_in_ocs(dim) {
        let mut wires = tessellate_dimension_inner(
            document,
            handle,
            &local,
            selected,
            entity_color,
            line_weight_px,
            anno_scale,
            selected_set,
            active_viewport,
            bg_color,
            None,
            world_per_pixel,
            reference_only,
        );
        for wire in &mut wires {
            map_wire_ocs_to_wcs(wire, normal);
        }
        return wires;
    }
    let name = handle.value().to_string();
    // (Baked-block fast path moved up into scene::tessellate_entity so the
    // recursive call goes through the LOD ladder, not the kernel path.)

    // Dimension geometry is f32. Tessellate it in a local frame anchored at a
    // point of the dimension, then fold the origin back in as a double-single
    // residual, so dimensions stay glued to their measured points on
    // UTM-scale drawings instead of drifting by up to half a metre.
    let source_dim = dim;
    let origin = dimension_precision_origin(dim);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dim, origin));
    let dim = shifted.as_ref().unwrap_or(dim);

    let source_style = dimension_source_style(document, dim);
    let effective_style = source_style.map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();

    // A positive style scale is fixed. A zero style scale uses the multiplier
    // resolved by the scene for the current annotation or viewport context.
    let dim_scale = style
        .map(|s| {
            if s.dimscale > 1e-6 {
                s.dimscale
            } else {
                anno_scale as f64
            }
        })
        .unwrap_or(1.0);

    let (
        dimasz_raw,
        dimexo,
        dimexe,
        dim_txt,
        dimtsz_raw,
        dimsah,
        dimse1,
        dimse2,
        dimsd1,
        dimsd2,
        dimdle,
        dimfxl,
        dimfxlon,
        dimsoxd,
        dimcen,
    ) = style
        .map(|s| {
            (
                s.dimasz * dim_scale,
                (s.dimexo * dim_scale) as f32,
                (s.dimexe * dim_scale) as f32,
                s.dimtxt * dim_scale,
                s.dimtsz * dim_scale,
                s.dimsah,
                s.dimse1,
                s.dimse2,
                s.dimsd1,
                s.dimsd2,
                (s.dimdle * dim_scale) as f32,
                (s.dimfxl * dim_scale) as f32,
                s.dimfxlon,
                s.dimsoxd,
                (s.dimcen * dim_scale) as f32,
            )
        })
        .unwrap_or((
            0.18, 0.0, 0.0, 2.5, 0.0, false, false, false, false, false, 0.0, 1.0, false, false,
            0.09,
        ));

    // Arrow selection precedence:
    //   1. DIMTSZ>0 → oblique tick (overrides arrow blocks).
    //   2. Radius and large-radius dimensions → DIMLDRBLK.
    //   3. DIMSAH false → DIMBLK on both ends.
    //   4. DIMSAH true  → DIMBLK1 (first end), DIMBLK2 (second end).
    // Unknown / NULL block handles fall back to ClosedFilled.
    let dimasz = (dimasz_raw as f32).max(0.001);
    let defer_arrow_hatches = {
        let name = dim.base().block_name.trim();
        let mut memo = std::collections::HashMap::new();
        !name.is_empty()
            && crate::scene::render_graph::block_contains_hatch(document, name, &mut memo)
    };
    let (arrow1, arrow2) = if dimtsz_raw > 1e-9 {
        let t = ArrowKind::Tick {
            size: (dimtsz_raw as f32).max(0.001),
            width: 0.0,
        };
        (t.clone(), t)
    } else if let Some(s) = style {
        if matches!(dim, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
            let handle = crate::entities::dim_override::handle(
                &dim.base().common.extended_data,
                crate::entities::dim_override::DIMLDRBLK,
            )
            .unwrap_or(s.dimldrblk);
            let arrow =
                arrow_from_block_with_deferred_hatch(document, handle, dimasz, defer_arrow_hatches);
            (arrow.clone(), arrow)
        } else if matches!(dim, Dimension::Diameter(_)) {
            let data = &dim.base().common.extended_data;
            let first =
                crate::entities::dim_override::handle(data, crate::entities::dim_override::DIMBLK1)
                    .unwrap_or(if dimsah { s.dimblk1 } else { s.dimblk });
            let second =
                crate::entities::dim_override::handle(data, crate::entities::dim_override::DIMBLK2)
                    .unwrap_or(if dimsah { s.dimblk2 } else { s.dimblk });
            (
                arrow_from_block_with_deferred_hatch(document, first, dimasz, defer_arrow_hatches),
                arrow_from_block_with_deferred_hatch(document, second, dimasz, defer_arrow_hatches),
            )
        } else if dimsah {
            (
                arrow_from_block_with_deferred_hatch(
                    document,
                    s.dimblk1,
                    dimasz,
                    defer_arrow_hatches,
                ),
                arrow_from_block_with_deferred_hatch(
                    document,
                    s.dimblk2,
                    dimasz,
                    defer_arrow_hatches,
                ),
            )
        } else {
            let a = arrow_from_block_with_deferred_hatch(
                document,
                s.dimblk,
                dimasz,
                defer_arrow_hatches,
            );
            (a.clone(), a)
        }
    } else {
        let a = ArrowKind::Triangle {
            size: dimasz,
            filled: true,
            size_mul: 1.0,
        };
        (a.clone(), a)
    };

    let text_layout = dimension_text_layout(dim, style, dim_txt, dim_scale, Some(document));
    let moved_text = dimtmove_one_text(dim, style, dim_txt, dim_scale, Some(document));
    // Text out on its own leader leaves the dimension line as DIMTMOVE 2
    // does: no line chasing it, no turn toward it.
    let text_movement = if moved_text
        .as_ref()
        .is_some_and(|moved| !moved.lines.is_empty())
    {
        2
    } else {
        style.map(|s| s.dimtmove).unwrap_or(0)
    };

    let mut geom = dimension_geometry(
        dim,
        &arrow1,
        &arrow2,
        DimLineParams {
            dimexo,
            dimexe,
            dimdle,
            dimfxl,
            dimfxlon,
            dimsoxd,
            dimcen,
            // Oblique-stroke and architectural-tick arrow blocks are ticks
            // too: they sit on the extension line and need no room of their
            // own, and DIMDLE applies to them. Given arrow-sized room, an
            // 18-long dimension flipped its strokes outside (#898).
            ticks: dimtsz_raw > 1e-9
                || (matches!(arrow1, ArrowKind::Tick { .. })
                    && matches!(arrow2, ArrowKind::Tick { .. })),
            arrow_len: dimasz,
            text_width: text_layout.width,
            text_height: dim_txt as f32,
            constraint: dynamic_constraint_dimension(dim),
            dimatfit: style.map(|s| s.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|s| s.dimtix),
            dimtofl: style.map(|s| s.dimtofl).unwrap_or(false),
            flip1: dim.base().flip_arrow1,
            flip2: dim.base().flip_arrow2,
            text_position: text_layout.position,
            horizontal_text: text_layout.horizontal,
            text_lifted: style.map(|s| s.dimtad).unwrap_or(1) != 0 || text_splits_at_line(dim),
            text_movement,
            text_break: text_layout.break_box,
            leader_anchor: stored_text_point(dim)
                .filter(|point| {
                    style.is_some_and(|s| s.dimtmove == 1)
                        || radius_hook_point(dim, style, *point, dim_txt as f64)
                })
                .map(vec3_local),
        },
        SuppressFlags {
            ext1: dimse1,
            ext2: dimse2,
            dim1: dimsd1,
            dim2: dimsd2,
        },
    );

    if !dimse1 {
        if let Some(points) = crate::scene::dimension_assoc::radial_extension_points(
            document,
            handle,
            dimexo as f64,
            dimexe as f64,
        ) {
            let points: Vec<Vec3> = points
                .into_iter()
                .map(|point| vec3_local(point_from_origin(point, origin)))
                .collect();
            add_polyline(&mut geom.ext_lines, &points);
        }
    }

    // DIMTMOVE=1 connects the dimension line to rendered text.
    if let Some(s) = style {
        if let Some(moved) = moved_text.as_ref() {
            for (start, end) in &moved.lines {
                add_segment(&mut geom.dim_lines, vec3_local(*start), vec3_local(*end));
            }
        } else if s.dimtmove == 1 {
            if let Some((anchor, txt, under_text)) = dimtmove_leader_endpoints(dim, text_layout) {
                let gap = dim_txt as f32 * 0.5;
                if under_text || (txt - anchor).length() > gap * 2.0 {
                    add_segment_with_text_break(
                        &mut geom.dim_lines,
                        anchor,
                        txt,
                        text_layout.break_box,
                    );
                }
            }
        }
        // DIMUPT governs interactive creation-time text placement; saved
        // geometry already carries the resulting position.
        let _ = s.dimupt;
        let _ = s.dimarcsym;
        // DIMUNIT is the obsolete pre-R2000 linear unit format; DIMLUNIT
        // supersedes it. Read but not honoured.
        let _ = s.dimunit;
    }
    if let Some(point) = dimension_jog_point(source_dim) {
        let jog_angle = style
            .map(|style| style.dimjogang as f32)
            .unwrap_or(std::f32::consts::FRAC_PI_4);
        // The jog is as tall as the style's jog height factor times the text
        // height: its two sides rise by tan(angle / 2) of their run.
        let jog_height = style.map_or(1.5, |style| {
            crate::scene::creation_style::dim_style_extras(document, style.handle).jog_height
        }) as f32
            * dim_txt as f32;
        let rise = (jog_angle * 0.5).tan().abs().max(0.35);
        apply_dimension_jog(
            &mut geom.dim_lines,
            vec3_local(point_from_origin(point, origin)),
            vec3_local(dim.base().normal),
            jog_height * 0.5 / rise,
            jog_angle,
        );
    }
    apply_dimension_breaks(document, handle, &mut geom.dim_lines, origin);
    apply_dimension_breaks(document, handle, &mut geom.ext_lines, origin);
    geom.origin = origin;
    // Dimension entity fields that the render path doesn't yet use but are
    // preserved on save:
    //   - base.insertion_point: legacy anchor reference; render uses
    //     text_middle_point + dim-line geometry instead.
    //   - base.block_name: generated anonymous block name for
    //     the dim graphics — we re-tessellate so don't need it.
    //   - base.version: DXF format marker (metadata only).
    let _ = (
        dim.base().insertion_point,
        &dim.base().block_name,
        dim.base().version,
    );

    // Per-spec colours: DIMCLRD (dim/arrows), DIMCLRE (ext), DIMCLRT (text).
    // 0=ByBlock and 256=ByLayer fall through to entity_color. Each honours the
    // per-object ACAD_DSTYLE override (codes 176/177/178) the Properties panel
    // reads and writes, so an edited colour renders without touching the style.
    let xd = &dim.base().common.extended_data;
    let dim_color = if selected {
        WireModel::SELECTED
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRD,
                style.map(|s| s.dimclrd).unwrap_or(0),
            ),
            entity_color,
        )
    };
    let ext_color = if selected {
        WireModel::SELECTED
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRE,
                style.map(|s| s.dimclre).unwrap_or(0),
            ),
            entity_color,
        )
    };
    let text_color = if selected {
        entity_color // text wire color set by inner tessellate; keep entity tint
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRT,
                style.map(|s| s.dimclrt).unwrap_or(0),
            ),
            entity_color,
        )
    };

    let snap_pts = dimension_snap_pts(source_dim);
    let key_vertices: Vec<[f64; 3]> = geom
        .dim_lines
        .iter()
        .chain(geom.ext_lines.iter())
        .copied()
        .filter(|p| !(p[0].is_nan() || p[1].is_nan() || p[2].is_nan()))
        // Still in the local frame here; `translate_wire_from_origin` folds
        // the origin in once, together with the wire's point buffers.
        .map(|[x, y, z]| [x as f64, y as f64, z as f64])
        .collect();

    // DIMLWD (dim line + arrows) and DIMLWE (extension lines). Negative
    // codes fall through to the entity's own resolved weight.
    let lw_dim = resolve_dim_lineweight_px(style.map(|s| s.dimlwd).unwrap_or(-2), line_weight_px);
    let lw_ext = resolve_dim_lineweight_px(style.map(|s| s.dimlwe).unwrap_or(-2), line_weight_px);

    // DIMLTEX (dim line) / DIMLTEX1 (ext1) / DIMLTEX2 (ext2) — linetype
    // handles → pattern. Looked up in document.line_types by handle.
    let lt_scale = document.header.linetype_scale as f32 * dim.base().common.linetype_scale as f32;
    let pattern = |line_type: Option<document::types::Handle>| {
        resolve_pattern_by_handle(
            document,
            line_type.unwrap_or(document::types::Handle::NULL),
            &dim.base().common,
            active_viewport,
            lt_scale,
        )
    };
    let (dim_pat_len, dim_pat) = pattern(style.map(|s| s.dimltex_handle));
    let (ext1_pat_len, ext1_pat) = pattern(style.map(|s| s.dimltex1_handle));
    let (ext2_pat_len, ext2_pat) = pattern(style.map(|s| s.dimltex2_handle));

    let mut wires = Vec::new();

    if !geom.ext_lines.is_empty() {
        // If ext1 and ext2 have different linetypes, split into two wires so
        // each can carry its own pattern. Otherwise emit as a single wire.
        let split = ext1_pat_len != ext2_pat_len || ext1_pat != ext2_pat;
        if split {
            let (ext1, ext2) = split_ext_lines(&geom.ext_lines);
            if !ext1.is_empty() {
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
                    color: ext_color,
                    selected,
                    aci: 0,
                    pattern_length: ext1_pat_len,
                    pattern: ext1_pat,
                    line_weight_px: lw_ext,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    geom: std::sync::Arc::new(crate::scene::WireGeom {
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        text_verts: Vec::new(),
                        points: ext1,
                        points_low: Vec::new(),
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        fill_tris: vec![],
                        fill_tris_low: Vec::new(),
                        ..crate::scene::WireGeom::default()
                    }),
                    ..Default::default()
                });
            }
            if !ext2.is_empty() {
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
                    color: ext_color,
                    selected,
                    aci: 0,
                    pattern_length: ext2_pat_len,
                    pattern: ext2_pat,
                    line_weight_px: lw_ext,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    geom: std::sync::Arc::new(crate::scene::WireGeom {
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        text_verts: Vec::new(),
                        points: ext2,
                        points_low: Vec::new(),
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        fill_tris: vec![],
                        fill_tris_low: Vec::new(),
                        ..crate::scene::WireGeom::default()
                    }),
                    ..Default::default()
                });
            }
        } else {
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
                color: ext_color,
                selected,
                aci: 0,
                pattern_length: ext1_pat_len,
                pattern: ext1_pat,
                line_weight_px: lw_ext,
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                geom: std::sync::Arc::new(crate::scene::WireGeom {
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
                    text_verts: Vec::new(),
                    points: geom.ext_lines,
                    points_low: Vec::new(),
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),
                    ..crate::scene::WireGeom::default()
                }),
                ..Default::default()
            });
        }
    }

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
        color: dim_color,
        selected,
        aci: 0,
        pattern_length: dim_pat_len,
        pattern: dim_pat,
        line_weight_px: lw_dim,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        geom: std::sync::Arc::new(crate::scene::WireGeom {
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            text_verts: Vec::new(),
            points: geom.dim_lines,
            points_low: Vec::new(),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices,
            fill_tris: geom.arrow_fill,
            // fill_tris_low intentionally empty: this fill renders on the top-level
            // path, where consumers (face3d_gpu, xclip) treat a short low half as
            // all-zero, so it draws at f32 precision (sub-metre error at UTM scale)
            // — not a crash. Follow-up: double-single-split via points_to_ds to
            // match emit_wire's paired fill path.
            fill_tris_low: Vec::new(),
            ..crate::scene::WireGeom::default()
        }),
        ..Default::default()
    });
    // Arrowheads and ticks: the dimension line's colour and weight, never
    // its dashes.
    if !geom.arrow_lines.is_empty() {
        if let Some(line) = wires.last() {
            let mut arrows = line.clone();
            arrows.points = std::mem::take(&mut geom.arrow_lines);
            arrows.pattern_length = 0.0;
            arrows.pattern = [0.0; 8];
            arrows.fill_tris = Vec::new();
            arrows.snap_pts = Vec::new();
            arrows.key_vertices = Vec::new();
            arrows.tangent_geoms = Vec::new();
            wires.push(arrows);
        }
    }

    if let Some(symbol) = style.and_then(|style| {
        arc_length_symbol_points(
            dim,
            Some(style),
            dim_txt,
            dim_scale,
            style.dimarcsym,
            Some(document),
        )
    }) {
        let mut points = Vec::new();
        add_polyline(&mut points, &symbol);
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
            color: if selected {
                WireModel::SELECTED
            } else {
                text_color
            },
            selected,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            geom: std::sync::Arc::new(crate::scene::WireGeom {
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                pick_tris: Vec::new(),
                pick_tris_low: Vec::new(),
                text_verts: Vec::new(),
                points,
                points_low: Vec::new(),
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: vec![],
                fill_tris: vec![],
                fill_tris_low: Vec::new(),
                ..crate::scene::WireGeom::default()
            }),
            ..Default::default()
        });
    }

    // DIMTFILL: 0=none, 1=drawing background (mask), 2=DIMTFILLCLR.
    if let Some(s) = style {
        if s.dimtfill == 1 || s.dimtfill == 2 {
            if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale, Some(document)) {
                let fill_color = if selected {
                    WireModel::SELECTED
                } else if s.dimtfill == 1 {
                    // Drawing-background fill: mask out geometry behind the text.
                    bg_color
                } else {
                    let c = AcadColor::from_index(s.dimtfillclr);
                    aci_to_rgba(&c)
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
                    color: fill_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px: 1.0,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    geom: std::sync::Arc::new(crate::scene::WireGeom {
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        text_verts: Vec::new(),
                        points: vec![],
                        points_low: Vec::new(),
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        fill_tris: rect,
                        // fill_tris_low intentionally empty: this fill renders on the
                        // top-level path, where consumers (face3d_gpu, xclip) treat a
                        // short low half as all-zero, so it draws at f32 precision
                        // (sub-metre error at UTM scale) — not a crash. Follow-up:
                        // double-single-split via points_to_ds to match emit_wire.
                        fill_tris_low: Vec::new(),
                        ..crate::scene::WireGeom::default()
                    }),
                    ..Default::default()
                });
            }
        }
    }

    // DIMINSPECT: label, value and rate in their own frame.
    let inspection = crate::entities::dim_inspect::read(&dim.base().common.extended_data);
    // A negative DIMGAP denotes the Basic tolerance display: frame the
    // dimension text while keeping the absolute gap as the frame margin.
    if inspection.is_none() && style.is_some_and(|s| s.dimgap < 0.0) {
        if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale, Some(document)) {
            let p1 = rect[0];
            let p2 = rect[1];
            let p3 = rect[2];
            let p4 = rect[5];
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
                color: if selected {
                    WireModel::SELECTED
                } else {
                    text_color
                },
                selected,
                aci: 0,
                pattern_length: 0.0,
                pattern: [0.0; 8],
                line_weight_px: 1.0,
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                geom: std::sync::Arc::new(crate::scene::WireGeom {
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
                    text_verts: Vec::new(),
                    points: vec![p1, p2, p2, p3, p3, p4, p4, p1],
                    points_low: Vec::new(),
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),
                    ..crate::scene::WireGeom::default()
                }),
                ..Default::default()
            });
        }
    }

    // Everything above is local-frame geometry: fold the origin back in as a
    // double-single residual. The synthesised text below is built from the
    // original f64 position, so it stays world-space and must not be shifted.
    if origin != Vector3::ZERO {
        for wire in &mut wires {
            translate_wire_from_origin(wire, origin);
        }
    }

    if let Some(mut synth_text_entity) =
        dimension_text_entity(source_dim, dim_txt, style, document, dim_scale)
    {
        // The glyphs take their colour from the text entity itself, which
        // copies the dimension's (usually ByLayer); an explicit DIMCLRT has to
        // be on the entity or the layer colour wins. (#898)
        let text_index = dim_color_index(
            xd,
            crate::entities::dim_override::DIMCLRT,
            style.map(|s| s.dimclrt).unwrap_or(0),
        );
        if (1..=255).contains(&text_index) {
            synth_text_entity.common_mut().color = AcadColor::from_index(text_index);
        }
        // Tolerance Text rendered separately so DIMTFAC scales its height
        // and DIMTOLJ aligns it vertically against the primary text.
        let tol_entity = dimension_tolerance_entity(source_dim, style, &synth_text_entity, dim_txt);
        let text_entities = match &inspection {
            Some(inspection) => crate::entities::dim_inspect::layout(
                inspection,
                synth_text_entity,
                document,
                style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale,
            ),
            None => vec![synth_text_entity],
        };
        // Route synthesised dim text through tessellate_entity so the
        // baseline/greek/full LOD ladder applies (zoom-out behaviour
        // matches top-level Text / MText). The text already has dim_scale
        // baked into its height, so anno_scale stays 1.0.
        for text_entity in &text_entities {
            let text_wires = crate::scene::tessellate_entity_dim_text(
                document,
                selected_set,
                active_viewport,
                bg_color,
                1.0,
                text_entity,
                view_aabb,
                world_per_pixel,
                text_color,
                reference_only,
            );
            for mut w in text_wires {
                w.name = name.clone();
                wires.push(w);
            }
        }

        if let Some(tol_entity_e) = tol_entity {
            let tol_wires = crate::scene::tessellate_entity_dim_text(
                document,
                selected_set,
                active_viewport,
                bg_color,
                1.0,
                &tol_entity_e,
                view_aabb,
                world_per_pixel,
                text_color,
                reference_only,
            );
            for mut w in tol_wires {
                w.name = name.clone();
                wires.push(w);
            }
        }
    }

    wires
}
