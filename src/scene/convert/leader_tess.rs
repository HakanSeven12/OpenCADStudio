//! How a leader draws: its path, arrowhead and hook line, styled by its
//! dimension style.

use document::entities::Leader;
use crate::display::wire_model::TangentGeom;
use crate::entities::leader::draws_hookline;
use glam::Vec3;

/// Per-entity tessellation entry for `Leader`. Lives here so all leader
/// tess code stays alongside the entity definition. Cross-entity dim
/// machinery (arrow shapes, `DimGeom`) lives in `scene::convert::tessellate` and
/// is reused via the dim arrow emitter so the leader matches the active
/// DIMSTYLE.
pub trait LeaderTess {
    fn tessellate(
        &self,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
    ) -> crate::display::wire_model::WireModel;
}

impl LeaderTess for Leader {
    fn tessellate(
        &self,
        document: &document::Document,
        handle: document::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
    ) -> crate::display::wire_model::WireModel {
        use crate::entities::dim_override as dov;
        use crate::display::dim_geom::{append_arrow, ArrowKind, DimGeom};
        use crate::scene::convert::tessellate::arrow_from_block;
        use crate::display::wire_model::WireModel;
        let xd = &self.common.extended_data;
        // Dim-line colour: a per-object ACAD_DSTYLE override (code 176, an ACI
        // index) wins over the assigned dim style's DIMCLRD; ByLayer / ByBlock
        // (0 / 256) and no setting fall through to the entity colour.
        let color = if selected {
            WireModel::SELECTED
        } else {
            let dim_clr = dov::int(xd, dov::DIMCLRD).or_else(|| {
                document
                    .dim_styles
                    .iter()
                    .find(|s| {
                        s.name.eq_ignore_ascii_case(&self.dimension_style)
                            || (self.dimension_style.trim().is_empty()
                                && s.name.eq_ignore_ascii_case("Standard"))
                    })
                    .map(|s| s.dimclrd)
            });
            match dim_clr {
                Some(idx) if idx != 0 && idx != 256 => {
                    crate::display::tess_util::aci_to_rgba(
                        &document::types::Color::from_index(idx),
                    )
                }
                _ => entity_color,
            }
        };
        // A concrete DIMLWD override sets the leader line's weight; ByLayer /
        // ByBlock / Default and no override keep the resolved weight passed in.
        let line_weight_px = match dov::int(xd, dov::DIMLWD) {
            Some(lwd) if lwd >= 0 => crate::scene::view::render::lineweight_to_px(
                &document::types::LineWeight::from_value(lwd),
            ),
            _ => line_weight_px,
        };
        let name = handle.value().to_string();
        let p3 = |v: &document::types::Vector3| -> [f32; 3] {
            [(v.x) as f32, (v.y) as f32, (v.z) as f32]
        };

        let verts = &self.vertices;

        if verts.len() < 2 {
            return WireModel {
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
                color,
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
                    points: vec![],
                    points_low: Default::default(),
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    ..crate::display::wire_model::WireGeom::default()
                }),
                ..Default::default()
            };
        }

        // Build all geometry in absolute f64 and RTE-split at the end. At UTM
        // coordinates a raw `v.x as f32` quantises to ~0.5 units, which flattens
        // the oblique arrow tick into a horizontal line and jitters the path.
        let origin = [verts[0].x, verts[0].y, verts[0].z];
        let mut points: Vec<[f64; 3]> = verts.iter().map(|v| [v.x, v.y, v.z]).collect();
        let mut tangents: Vec<TangentGeom> = Vec::new();
        let key_vertices: Vec<[f64; 3]> = verts.iter().map(|v| [v.x, v.y, v.z]).collect();
        let mut fill_tris: Vec<[f64; 3]> = Vec::new();

        for i in 0..verts.len().saturating_sub(1) {
            tangents.push(TangentGeom::Line {
                p1: p3(&verts[i]),
                p2: p3(&verts[i + 1]),
            });
        }

        if self.arrow_enabled {
            // Resolve the active dim style → DIMLDRBLK to pick the arrow shape.
            // DIMASZ × DIMSCALE drives the size when available; otherwise fall
            // back to the legacy text-height heuristic.
            let style = document.dim_styles.iter().find(|s| {
                s.name.eq_ignore_ascii_case(&self.dimension_style)
                    || (self.dimension_style.trim().is_empty()
                        && s.name.eq_ignore_ascii_case("Standard"))
            });
            // Each of DIMSCALE / DIMASZ / DIMLDRBLK prefers a per-object override
            // over the style, so an edited leader arrow renders at its new size,
            // scale and shape.
            let dim_scale = dov::real(xd, dov::DIMSCALE)
                .filter(|v| *v > 1e-6)
                .or_else(|| style.map(|s| s.dimscale).filter(|v| *v > 1e-6))
                .unwrap_or(anno_scale as f64);
            let ovr_asz = dov::real(xd, dov::DIMASZ);
            let arrow_size = match (ovr_asz, style) {
                (Some(a), _) => (a * dim_scale) as f32,
                (None, Some(s)) => (s.dimasz * dim_scale) as f32,
                (None, None) => (self.text_height as f32).max(1.0) * anno_scale,
            };
            let arrow_blk = dov::handle(xd, dov::DIMLDRBLK).or_else(|| style.map(|s| s.dimldrblk));
            let arrow = match arrow_blk {
                Some(h) => arrow_from_block(document, h, arrow_size.max(0.001)),
                None => ArrowKind::Triangle {
                    size: arrow_size.max(0.001),
                    filled: true,
                    size_mul: 1.0,
                },
            };

            let tip = &verts[0];
            let next = &verts[1];
            let dx = (next.x - tip.x) as f32;
            let dy = (next.y - tip.y) as f32;
            let len = (dx * dx + dy * dy).sqrt().max(1e-9);
            let dir = Vec3::new(dx / len, dy / len, 0.0);
            // Omit the arrowhead when it is larger than the first leader segment
            // it would sit on: an oversized arrow on a short pointer is
            // suppressed, not drawn overshooting the segment. Without this an
            // annotation-scaled arrow (DIMASZ sized for a bigger plot) paints a
            // stray stroke longer than the leader itself.
            if arrow_size <= len {
                // Emit the arrow around a local origin (the tip) so
                // `append_arrow`'s f32 math stays precise, then lift each point
                // back to absolute f64. Reuse the dim arrow emitter so the
                // leader shape matches the DIMSTYLE in use (Closed Filled by
                // default, Dot, Tick, …).
                let mut arrow_geom = DimGeom::new();
                append_arrow(&mut arrow_geom, Vec3::ZERO, dir, &arrow);
                let lift = |p: &[f32; 3]| -> [f64; 3] {
                    [
                        p[0] as f64 + origin[0],
                        p[1] as f64 + origin[1],
                        p[2] as f64 + origin[2],
                    ]
                };
                if !arrow_geom.dim_lines.is_empty() {
                    points.push([f64::NAN; 3]);
                    points.extend(arrow_geom.dim_lines.iter().map(&lift));
                }
                fill_tris.extend(arrow_geom.arrow_fill.iter().map(&lift));
            }
        }

        if draws_hookline(self) {
            let last = verts.last().unwrap();
            let prev = &verts[verts.len() - 2];
            let sign = if (last.x - prev.x) >= 0.0 {
                1.0_f64
            } else {
                -1.0_f64
            };
            let style = document.dim_styles.iter().find(|s| {
                s.name.eq_ignore_ascii_case(&self.dimension_style)
                    || (self.dimension_style.trim().is_empty()
                        && s.name.eq_ignore_ascii_case("Standard"))
            });
            let dim_scale = dov::real(xd, dov::DIMSCALE)
                .filter(|v| *v > 1e-6)
                .or_else(|| style.map(|s| s.dimscale).filter(|v| *v > 1e-6))
                .unwrap_or(anno_scale as f64);
            // Hook length: DIMASZ, like the arrowhead, when a style resolves;
            // the legacy text-height heuristic otherwise.
            let mut land_len = match dov::real(xd, dov::DIMASZ).or_else(|| style.map(|s| s.dimasz))
            {
                Some(a) => a * dim_scale,
                None => self.text_height * 1.5 * anno_scale as f64,
            };
            // With DIMTAD "text above", the annotation sits on top of the hook
            // and the hook underlines it: extend to the far edge of the MTEXT's
            // laid-out extents.
            let dimtad = dov::int(xd, dov::DIMTAD)
                .or_else(|| style.map(|s| s.dimtad))
                .unwrap_or(0);
            if dimtad != 0 {
                let mt = match document.get_entity(self.annotation_handle) {
                    Some(document::entities::EntityType::MText(mt)) => {
                        Some((mt.extents_width, mt.insertion_point.x))
                    }
                    _ => None,
                };
                // Underline width: the annotation MTEXT's laid-out extents when
                // known, otherwise the leader's own stored annotation width
                // (DXF code 41) — a DXF-loaded MTEXT often has no computed
                // extents (extents_width == 0), which left the hook too short.
                let width = mt
                    .map(|(w, _)| w)
                    .filter(|w| *w > 0.0)
                    .unwrap_or(self.text_width);
                if width > 0.0 {
                    let ins_x = mt.map(|(_, x)| x).unwrap_or(last.x);
                    let to_far_edge = sign * (ins_x - last.x) + width;
                    land_len = land_len.max(to_far_edge);
                }
            }
            points.push([f64::NAN; 3]);
            points.push([last.x, last.y, last.z]);
            points.push([last.x + sign * land_len, last.y, last.z]);
        }

        // Split absolute f64 into the render/eye double-single so the GPU keeps
        // full precision at UTM coordinates (empty low bufs = absolute f32).
        let (points, points_low) = crate::scene::convert::tessellate::points_to_ds(points);
        let (fill_tris, fill_tris_low) = crate::scene::convert::tessellate::points_to_ds(fill_tris);

        WireModel {
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
            color,
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
                points,
                points_low: points_low.into(),
                snap_pts: vec![],
                tangent_geoms: tangents,
                key_vertices,
                surfaces: crate::display::wire_model::WireSurfaces {
                    fill_tris,
                    fill_tris_low,
                    ..Default::default()
                }
                .into_optional(),
                ..crate::display::wire_model::WireGeom::default()
            }),
            ..Default::default()
        }
    }
}
