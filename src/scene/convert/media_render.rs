//! How underlays and raster images draw as outlines and frames: the parts
//! of their display that read the referenced file.

use document::entities::{RasterImage, Underlay};
use glam::DVec3;
use crate::display::render_entity::{GlyphRun, RenderEntity, RenderObject, TextStroke};
use crate::display::wire_model::SnapHint;
use crate::entities::raster_image::*;
use crate::entities::underlay::*;
use crate::entities::text_support::{resolve_text_style, text_local_bounds};
use crate::entities::traits::RenderConvertible;
use crate::text::lff;

impl RenderConvertible for Underlay {
    fn to_render(&self, document: &document::Document) -> Option<RenderEntity> {
        let origin = v3(&self.insertion_point);
        let insertion_snap = (
            DVec3::new(
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
            ),
            SnapHint::Insertion,
        );
        let quad = page_quad(self, document);
        let clip: Vec<[f64; 3]> = if is_clipped(self) {
            clip_polygon_local(self)
                .into_iter()
                .map(|p| local_to_world(self, p))
                .collect()
        } else {
            Vec::new()
        };

        // The outline is the clip boundary when clipped (plus the page frame
        // for an outside clip), else the page frame.
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let ring = |poly: &[[f64; 3]], pts: &mut Vec<[f64; 3]>| {
            if !pts.is_empty() {
                pts.push([f64::NAN; 3]);
            }
            pts.extend_from_slice(poly);
            pts.push(poly[0]);
        };
        if !clip.is_empty() {
            ring(&clip, &mut pts);
            if self.clip_inverted {
                if let Some(q) = quad {
                    ring(&q, &mut pts);
                }
            }
        } else if let Some(q) = quad {
            ring(&q, &mut pts);
        }
        if pts.is_empty() {
            return Some(RenderEntity {
                pick_tris: Vec::new(),
                object: RenderObject::Lines(cross_wire(origin, 1.0)),
                snap_pts: vec![insertion_snap],
                tangent_geoms: vec![],
                key_vertices: vec![origin],
                fill_tris: vec![],
            });
        }

        // Picked on its frame only; the page itself selects nothing.
        let key_ring: Vec<[f64; 3]> = if !clip.is_empty() && !self.clip_inverted {
            clip.clone()
        } else {
            quad.map(|q| q.to_vec()).unwrap_or_default()
        };
        let mut snap_pts = vec![insertion_snap];
        snap_pts.extend(crate::scene::model::pdf_vector::underlay_snap_points(
            self, document,
        ));
        Some(RenderEntity {
            pick_tris: vec![],
            object: RenderObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices: key_ring,
            fill_tris: vec![],
        })
    }
}

impl RenderConvertible for RasterImage {
    fn to_render(&self, document: &document::Document) -> Option<RenderEntity> {
        let corners = world_corners(self.corners());

        // Helper: pixel-space → world-space point.
        let ox = self.insertion_point.x;
        let oy = self.insertion_point.y;
        let oz = self.insertion_point.z;
        let px_to_world = |px: f64, py: f64| -> [f64; 3] {
            [
                ox + self.u_vector.x * px + self.v_vector.x * py,
                oy + self.u_vector.y * px + self.v_vector.y * py,
                oz + self.u_vector.z * px + self.v_vector.z * py,
            ]
        };

        // Diagonals are the BROKEN-reference placeholder; a resolvable image
        // draws its pixels inside the frame, so the X would scribble over it.
        // One probe for the whole conversion: the outline below and the
        // placeholder branch further down must agree on a single answer.
        let path = self.file_path.trim();
        let resolvable =
            path.is_empty() || crate::scene::model::image_model::resolve_image(path).is_some();
        // Outline: the clip boundary, or — when it hides its inside — the
        // frame with the boundary as a hole. Must match the raster's own clip
        // triangulation in `ImageModel` so outline and pixels align.
        let pts = match image_clip_polygon(self) {
            Some(poly) => {
                let mut ring: Vec<[f64; 3]> =
                    poly.iter().map(|p| px_to_world(p[0], p[1])).collect();
                ring.push(ring[0]);
                if self.clip_boundary.clip_mode == document::entities::ClipMode::Inside {
                    let mut frame = image_wire(corners, false);
                    frame.push([f64::NAN; 3]);
                    frame.extend(ring);
                    frame
                } else {
                    ring
                }
            }
            None => image_wire(corners, !resolvable),
        };

        // A raster OCS can display renders its pixels (built separately) inside
        // this frame — just draw the frame/clip outline. A reference it cannot
        // resolve (an offline/broken URL, a missing or renamed file) gets the
        // usual broken-reference treatment: the frame plus the saved path drawn
        // as text, so the user sees WHICH reference is unresolved instead of an
        // empty box. `resolve_image` is memoised and shared with the raster
        // loader, so a URL that fetches online is treated as resolvable (no
        // placeholder — the image shows) while an offline one falls back to the
        // path text, and neither is fetched twice. The scene's text-cache
        // classifier runs the same probe, so erasing this placeholder drops its
        // glyphs from the text buffer instead of leaving them on screen.
        if resolvable {
            return Some(RenderEntity {
                // Picked on its frame only (shown or hidden), like the other
                // framed objects: a click inside the picture selects nothing.
                pick_tris: vec![],
                object: RenderObject::Lines(pts),
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: corners.to_vec(),
                fill_tris: vec![],
            });
        }

        // ── Unresolved reference: frame outline + path text, image colour ──
        let ins = [self.insertion_point.x, self.insertion_point.y];
        // Boundary → run-less stroke groups (local to `ins`), split on the
        // NaN gaps the wire uses to separate disjoint segments.
        let mut boundary: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut seg: Vec<[f32; 2]> = Vec::new();
        for p in &pts {
            if p[0].is_nan() {
                if seg.len() >= 2 {
                    boundary.push(std::mem::take(&mut seg));
                } else {
                    seg.clear();
                }
            } else {
                seg.push([(p[0] - ins[0]) as f32, (p[1] - ins[1]) as f32]);
            }
        }
        if seg.len() >= 2 {
            boundary.push(seg);
        }

        let mut groups: Vec<TextStroke> = vec![TextStroke {
            strokes: boundary,
            origin: ins,
            color: None,
            fill_tris: vec![],
            plane: None,
            run: None,
        }];

        // Place the path text centred in the frame, sized to span ~90% of the
        // frame width (capped so it also fits vertically).
        let c0 = corners[0];
        let c2 = corners[2];
        let uw = [corners[1][0] - c0[0], corners[1][1] - c0[1]];
        let vh = [corners[3][0] - c0[0], corners[3][1] - c0[1]];
        let frame_w = (uw[0] * uw[0] + uw[1] * uw[1]).sqrt();
        let frame_h = (vh[0] * vh[0] + vh[1] * vh[1]).sqrt();
        if frame_w > 1e-9 && frame_h > 1e-9 {
            let u_hat = [uw[0] / frame_w, uw[1] / frame_w];
            let v_hat = [vh[0] / frame_h, vh[1] / frame_h];
            let rotation = (u_hat[1] as f32).atan2(u_hat[0] as f32);
            let font = resolve_text_style("", document).font_name;
            // advance at height 1.0 → width per unit height.
            let adv = text_local_bounds(&font, path, 1.0, 1.0, 0.0)
                .map(|b| b.advance)
                .filter(|a| *a > 1e-3)
                .unwrap_or(0.6 * path.chars().count().max(1) as f32);
            let height = ((frame_w as f32 * 0.9) / adv)
                .min(frame_h as f32 * 0.5)
                .max(frame_h as f32 * 0.02);
            let text_w = (adv * height) as f64;
            let center = [(c0[0] + c2[0]) * 0.5, (c0[1] + c2[1]) * 0.5];
            // Shift the baseline-left origin so the run is centred both ways.
            let origin = [
                center[0] - u_hat[0] * text_w * 0.5 - v_hat[0] * height as f64 * 0.35,
                center[1] - u_hat[1] * text_w * 0.5 - v_hat[1] * height as f64 * 0.35,
            ];
            let (strokes, fill_tris) =
                lff::tessellate_text_ex([0.0, 0.0], height, rotation, 1.0, 0.0, &font, path);
            groups.push(TextStroke {
                strokes,
                origin,
                color: None,
                fill_tris,
                plane: None,
                run: Some(GlyphRun {
                    text: path.to_string(),
                    font,
                    height,
                    rotation,
                    width_factor: 1.0,
                    oblique: 0.0,
                    tracking: 1.0,
                    bold: false,
                    aci: None,
                }),
            });
        }

        Some(RenderEntity {
            pick_tris: vec![],
            object: RenderObject::Text(groups),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: corners.to_vec(),
            fill_tris: vec![],
        })
    }
}
