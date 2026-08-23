use acadrust::entities::{RasterImage, Wipeout};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::text_support::{resolve_text_style, text_local_bounds};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{GlyphRun, TextStroke, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::scene::text::lff;

// ── Shared geometry helpers ───────────────────────────────────────────────────

/// Compute the four world-space corners of an image/wipeout from its
/// insertion_point, u_vector, v_vector and pixel size.
///
/// Returns (p0, p1, p2, p3) in counter-clockwise order:
///   p0 = origin
///   p1 = origin + U*W
///   p2 = origin + U*W + V*H
///   p3 = origin + V*H
fn image_corners(
    origin: &acadrust::types::Vector3,
    u: &acadrust::types::Vector3,
    v: &acadrust::types::Vector3,
    w: f64,
    h: f64,
) -> [[f64; 3]; 4] {
    let ox = origin.x;
    let oy = origin.y;
    let oz = origin.z;
    let ux = u.x * w;
    let uy = u.y * w;
    let uz = u.z * w;
    let vx = v.x * h;
    let vy = v.y * h;
    let vz = v.z * h;

    [
        [ox, oy, oz],
        [ox + ux, oy + uy, oz + uz],
        [ox + ux + vx, oy + uy + vy, oz + uz + vz],
        [ox + vx, oy + vy, oz + vz],
    ]
}

/// Rectangle border + X diagonals — used as a placeholder for images.
fn image_wire(corners: [[f64; 3]; 4], with_x: bool) -> Vec<[f64; 3]> {
    let [p0, p1, p2, p3] = corners;
    let mut pts = vec![p0, p1, p2, p3, p0];
    if with_x {
        pts.push([f64::NAN; 3]);
        pts.push(p0);
        pts.push(p2);
        pts.push([f64::NAN; 3]);
        pts.push(p1);
        pts.push(p3);
    }
    pts
}

fn reflect_vec3(vx: &mut f64, vy: &mut f64, ax: f64, ay: f64, len2: f64) {
    let dot = *vx * ax + *vy * ay;
    *vx = 2.0 * dot * ax / len2 - *vx;
    *vy = 2.0 * dot * ay / len2 - *vy;
}

// ── RasterImage ───────────────────────────────────────────────────────────────

impl RenderConvertible for RasterImage {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let corners = image_corners(
            &self.insertion_point,
            &self.u_vector,
            &self.v_vector,
            self.size.x,
            self.size.y,
        );

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

        // Clip-boundary Y is in image raster space (row 0 = top, Y down); the
        // image's v-vector points up, so flip each vertex's Y (`ih - y`) to
        // place the boundary where AutoCAD draws it. Must match the raster's
        // own clip triangulation in `ImageModel` so outline and pixels align.
        let ih = self.size.y;
        // Diagonals are the BROKEN-reference placeholder; a resolvable image
        // draws its pixels inside the frame, so the X would scribble over it.
        let path_probe = self.file_path.trim();
        let resolvable = path_probe.is_empty()
            || crate::scene::model::image_model::resolve_image(path_probe).is_some();
        let pts = if self.clipping_enabled {
            let cb = &self.clip_boundary;
            match cb.clip_type {
                acadrust::entities::ClipType::Polygonal if cb.vertices.len() >= 3 => {
                    let mut poly: Vec<[f64; 3]> =
                        cb.vertices.iter().map(|v| px_to_world(v.x, ih - v.y)).collect();
                    if let Some(&first) = poly.first() {
                        poly.push(first);
                    }
                    poly
                }
                acadrust::entities::ClipType::Rectangular if cb.vertices.len() >= 2 => {
                    let v0 = &cb.vertices[0];
                    let v1 = &cb.vertices[1];
                    let (xa, xb) = (v0.x.min(v1.x), v0.x.max(v1.x));
                    let (y0, y1) = (ih - v0.y, ih - v1.y);
                    let (ya, yb) = (y0.min(y1), y0.max(y1));
                    let c0 = px_to_world(xa, ya);
                    let c1 = px_to_world(xb, ya);
                    let c2 = px_to_world(xb, yb);
                    let c3 = px_to_world(xa, yb);
                    vec![c0, c1, c2, c3, c0]
                }
                _ => image_wire(corners, !resolvable),
            }
        } else {
            image_wire(corners, !resolvable)
        };

        // A raster OCS can display renders its pixels (built separately) inside
        // this frame — just draw the frame/clip outline. A reference it cannot
        // resolve (an offline/broken URL, a missing or renamed file) gets
        // AutoCAD's broken-reference treatment: the frame plus the saved path
        // drawn as text, so the user sees WHICH reference is unresolved instead
        // of an empty box. `resolve_image` is memoised and shared with the
        // raster loader, so a URL that fetches online is treated as resolvable
        // (no placeholder — the image shows) while an offline one falls back to
        // the path text, and neither is fetched twice.
        let path = self.file_path.trim();
        let resolvable =
            path.is_empty() || crate::scene::model::image_model::resolve_image(path).is_some();
        if resolvable {
            return Some(RenderEntity {
                // Interior pick surface: the image selects on a click anywhere
                // inside its frame, not just on the border.
                pick_tris: crate::entities::common::quad_pick_tris(&corners),
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
                run: Some(GlyphRun {
                    text: path.to_string(),
                    font,
                    height,
                    rotation,
                    width_factor: 1.0,
                    oblique: 0.0,
                    tracking: 1.0,
                    bold: false,
                }),
            });
        }

        Some(RenderEntity {
            pick_tris: crate::entities::common::quad_pick_tris(&corners),
            object: RenderObject::Text(groups),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: corners.to_vec(),
            fill_tris: vec![],
        })
    }
}

impl Grippable for RasterImage {
    fn grips(&self) -> Vec<GripDef> {
        let corners = image_corners(
            &self.insertion_point,
            &self.u_vector,
            &self.v_vector,
            self.size.x,
            self.size.y,
        );
        vec![
            square_grip(0, glam::DVec3::from(corners[0])),
            center_grip(1, glam::DVec3::from(corners[1])),
            center_grip(2, glam::DVec3::from(corners[2])),
            center_grip(3, glam::DVec3::from(corners[3])),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
        // Corner grips 1-3 are display-only (resizing changes u/v vectors,
        // which requires careful normalization — deferred).
    }
}

impl PropertyEditable for RasterImage {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let rotation_deg = self.u_vector.y.atan2(self.u_vector.x).to_degrees();
        let scale = self.u_vector.length();
        let show_image = self.flags.contains(acadrust::entities::ImageDisplayFlags::SHOW_IMAGE);
        let show_clipped = self
            .flags
            .contains(acadrust::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY);
        let clip_inverted = self.clip_boundary.clip_mode == acadrust::entities::ClipMode::Inside;
        let transparency = format!("{:.0}%", self.common.transparency.as_percent() * 100.0);
        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Position X").as_ref(), "ri_ox", self.insertion_point.x),
                    edit(t!("Position Y").as_ref(), "ri_oy", self.insertion_point.y),
                    edit(t!("Position Z").as_ref(), "ri_oz", self.insertion_point.z),
                    ro(t!("Rotation").as_ref(), "ri_rotation", format!("{:.4}", rotation_deg)),
                    ro(t!("Width").as_ref(), "ri_width", format!("{:.4}", self.width())),
                    ro(t!("Height").as_ref(), "ri_height", format!("{:.4}", self.height())),
                    ro(t!("Scale").as_ref(), "ri_scale", format!("{:.4}", scale)),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(t!("Name").as_ref(), "ri_name", self.file_name().to_string()),
                    edit(t!("Brightness").as_ref(), "ri_bright", self.brightness as f64),
                    edit(t!("Contrast").as_ref(), "ri_contrast", self.contrast as f64),
                    edit(t!("Fade").as_ref(), "ri_fade", self.fade as f64),
                    ro(t!("Transparency").as_ref(), "ri_transparency", transparency),
                    Property {
                        label: t!("Show image").into_owned(),
                        field: "ri_show_image",
                        value: PropValue::BoolToggle {
                            field: "ri_show_image",
                            value: show_image,
                        },
                    },
                    Property {
                        label: t!("Show clipped").into_owned(),
                        field: "ri_show_clipped",
                        value: PropValue::BoolToggle {
                            field: "ri_show_clipped",
                            value: show_clipped,
                        },
                    },
                    Property {
                        label: t!("Clip inverted").into_owned(),
                        field: "ri_clip_inverted",
                        value: PropValue::BoolToggle {
                            field: "ri_clip_inverted",
                            value: clip_inverted,
                        },
                    },
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ri_show_image" => {
                let on = if value == "toggle" {
                    !self.flags.contains(acadrust::entities::ImageDisplayFlags::SHOW_IMAGE)
                } else {
                    value == "true"
                };
                self.set_visible(on);
                return;
            }
            "ri_show_clipped" => {
                let on = if value == "toggle" {
                    !self
                        .flags
                        .contains(acadrust::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY)
                } else {
                    value == "true"
                };
                if on {
                    self.flags |= acadrust::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
                } else {
                    self.flags &= !acadrust::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
                }
                return;
            }
            "ri_clip_inverted" => {
                let on = if value == "toggle" {
                    self.clip_boundary.clip_mode != acadrust::entities::ClipMode::Inside
                } else {
                    value == "true"
                };
                self.clip_boundary.clip_mode = if on {
                    acadrust::entities::ClipMode::Inside
                } else {
                    acadrust::entities::ClipMode::Outside
                };
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "ri_ox" => self.insertion_point.x = v,
            "ri_oy" => self.insertion_point.y = v,
            "ri_oz" => self.insertion_point.z = v,
            "ri_bright" => self.brightness = v.clamp(0.0, 100.0) as u8,
            "ri_contrast" => self.contrast = v.clamp(0.0, 100.0) as u8,
            "ri_fade" => self.fade = v.clamp(0.0, 100.0) as u8,
            _ => {}
        }
    }
}

impl Transformable for RasterImage {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                reflect_vec3(&mut entity.u_vector.x, &mut entity.u_vector.y, ax, ay, len2);
                reflect_vec3(&mut entity.v_vector.x, &mut entity.v_vector.y, ax, ay, len2);
            }
        });
    }
}

// ── Wipeout ───────────────────────────────────────────────────────────────────

fn wipeout_is_polygonal(wipeout: &Wipeout) -> bool {
    wipeout.clipping_enabled
        && wipeout.clip_boundary_vertices.len() >= 3
        && matches!(
            wipeout.clip_type,
            acadrust::entities::WipeoutClipType::Polygonal
        )
}

fn wipeout_clip_to_world(wipeout: &Wipeout, point: &acadrust::types::Vector2) -> [f64; 3] {
    let x = point.x + wipeout.size.x * 0.5;
    let y = wipeout.size.y * 0.5 - point.y;
    wipeout_plane(wipeout).point_at([x, y])
}

fn wipeout_plane(wipeout: &Wipeout) -> cadkernel::space::Plane {
    cadkernel::space::Plane::from_axes(
        [
            wipeout.insertion_point.x,
            wipeout.insertion_point.y,
            wipeout.insertion_point.z,
        ],
        [
            wipeout.u_vector.x,
            wipeout.u_vector.y,
            wipeout.u_vector.z,
        ],
        [
            wipeout.v_vector.x,
            wipeout.v_vector.y,
            wipeout.v_vector.z,
        ],
    )
}

fn wipeout_boundary(wipeout: &Wipeout) -> Vec<[f64; 3]> {
    if wipeout_is_polygonal(wipeout) {
        wipeout
            .clip_boundary_vertices
            .iter()
            .map(|point| wipeout_clip_to_world(wipeout, point))
            .collect()
    } else {
        image_corners(
            &wipeout.insertion_point,
            &wipeout.u_vector,
            &wipeout.v_vector,
            wipeout.size.x,
            wipeout.size.y,
        )
        .to_vec()
    }
}

fn wipeout_pick_rings(wipeout: &Wipeout) -> Vec<Vec<[f64; 3]>> {
    let boundary = wipeout_boundary(wipeout);
    if wipeout_is_polygonal(wipeout)
        && matches!(
            wipeout.clip_mode,
            acadrust::entities::WipeoutClipMode::Inside
        )
    {
        vec![
            image_corners(
                &wipeout.insertion_point,
                &wipeout.u_vector,
                &wipeout.v_vector,
                wipeout.size.x,
                wipeout.size.y,
            )
            .to_vec(),
            boundary,
        ]
    } else {
        vec![boundary]
    }
}

fn wipeout_world_to_clip(wipeout: &Wipeout, world: [f64; 3]) -> Option<acadrust::types::Vector2> {
    let [x, y] = wipeout_plane(wipeout).project(world)?;
    Some(acadrust::types::Vector2::new(
        x - wipeout.size.x * 0.5,
        wipeout.size.y * 0.5 - y,
    ))
}

impl RenderConvertible for Wipeout {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let boundary = wipeout_boundary(self);
        let mut pts = boundary.clone();
        if let Some(&first) = pts.first() {
            pts.push(first);
        }

        Some(RenderEntity {
            pick_tris: cadkernel::space::polygon::triangulate_rings(
                &wipeout_pick_rings(self),
                cadkernel::geom2d::Tolerance::default(),
            ),
            object: RenderObject::Lines(pts),
            snap_pts: boundary
                .iter()
                .map(|point| (glam::DVec3::from(*point), SnapHint::Endpoint))
                .collect(),
            tangent_geoms: vec![],
            key_vertices: boundary,
            fill_tris: vec![],
        })
    }
}

impl Grippable for Wipeout {
    fn grips(&self) -> Vec<GripDef> {
        if wipeout_is_polygonal(self) {
            wipeout_boundary(self)
                .into_iter()
                .enumerate()
                .map(|(i, point)| {
                    if i == 0 {
                        square_grip(i, glam::DVec3::from(point))
                    } else {
                        center_grip(i, glam::DVec3::from(point))
                    }
                })
                .collect()
        } else {
            let corners = image_corners(
                &self.insertion_point,
                &self.u_vector,
                &self.v_vector,
                self.size.x,
                self.size.y,
            );
            vec![
                square_grip(0, glam::DVec3::from(corners[0])),
                center_grip(1, glam::DVec3::from(corners[1])),
                center_grip(2, glam::DVec3::from(corners[2])),
                center_grip(3, glam::DVec3::from(corners[3])),
            ]
        }
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if wipeout_is_polygonal(self) {
            if let Some(current) = self.clip_boundary_vertices.get(grip_id).cloned() {
                let current_world = wipeout_clip_to_world(self, &current);
                let new_w = match apply {
                    GripApply::Translate(d) => [
                        current_world[0] + d.x as f64,
                        current_world[1] + d.y as f64,
                        current_world[2] + d.z as f64,
                    ],
                    GripApply::Absolute(p) => [p.x as f64, p.y as f64, p.z as f64],
                };
                if let Some(clip) = wipeout_world_to_clip(self, new_w) {
                    self.clip_boundary_vertices[grip_id] = clip;
                }
            }
            return;
        }

        let corners = wipeout_boundary(self);
        let Some(current) = corners.get(grip_id).copied() else {
            return;
        };
        let target = match apply {
            GripApply::Translate(delta) => glam::DVec3::from(current) + delta,
            GripApply::Absolute(point) => point,
        };
        let u_hat = glam::DVec3::new(self.u_vector.x, self.u_vector.y, self.u_vector.z)
            .normalize_or_zero();
        let v_hat = glam::DVec3::new(self.v_vector.x, self.v_vector.y, self.v_vector.z)
            .normalize_or_zero();
        if u_hat == glam::DVec3::ZERO || v_hat == glam::DVec3::ZERO {
            return;
        }
        let resize_plane = cadkernel::space::Plane::from_axes(
            [0.0; 3],
            u_hat.to_array(),
            v_hat.to_array(),
        );
        let (width, height, insertion) = match grip_id {
            0 => {
                let fixed = glam::DVec3::from(corners[2]);
                let Some([width, height]) =
                    resize_plane.project_vector((fixed - target).to_array())
                else {
                    return;
                };
                (width, height, target)
            }
            1 => {
                let fixed = glam::DVec3::from(corners[3]);
                let Some([width, neg_height]) =
                    resize_plane.project_vector((target - fixed).to_array())
                else {
                    return;
                };
                let height = -neg_height;
                (width, height, fixed - v_hat * height)
            }
            2 => {
                let fixed = glam::DVec3::from(corners[0]);
                let Some([width, height]) =
                    resize_plane.project_vector((target - fixed).to_array())
                else {
                    return;
                };
                (width, height, fixed)
            }
            3 => {
                let fixed = glam::DVec3::from(corners[1]);
                let Some([neg_width, height]) =
                    resize_plane.project_vector((target - fixed).to_array())
                else {
                    return;
                };
                let width = -neg_width;
                (width, height, fixed - u_hat * width)
            }
            _ => return,
        };
        if width.abs() > 1e-9 && height.abs() > 1e-9 {
            self.insertion_point = acadrust::types::Vector3::new(
                insertion.x,
                insertion.y,
                insertion.z,
            );
            let sx = self.size.x.abs().max(1e-9);
            let sy = self.size.y.abs().max(1e-9);
            self.u_vector = acadrust::types::Vector3::new(
                u_hat.x * width / sx,
                u_hat.y * width / sx,
                u_hat.z * width / sx,
            );
            self.v_vector = acadrust::types::Vector3::new(
                v_hat.x * height / sy,
                v_hat.y * height / sy,
                v_hat.z * height / sy,
            );
        }
    }
}

impl PropertyEditable for Wipeout {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let show_image = self.flags.contains(acadrust::entities::WipeoutDisplayFlags::SHOW_IMAGE);
        let show_clipped = self
            .flags
            .contains(acadrust::entities::WipeoutDisplayFlags::USE_CLIPPING_BOUNDARY);
        let bg_transparency = self
            .flags
            .contains(acadrust::entities::WipeoutDisplayFlags::TRANSPARENCY_ON);
        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Position X").as_ref(), "wo_ox", self.insertion_point.x),
                    edit(t!("Position Y").as_ref(), "wo_oy", self.insertion_point.y),
                    edit(t!("Position Z").as_ref(), "wo_oz", self.insertion_point.z),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(t!("Show image").as_ref(), "wo_show_image", if show_image { t!("Yes") } else { t!("No") }),
                    ro(t!("Show clipped").as_ref(), "wo_show_clipped", if show_clipped { t!("Yes") } else { t!("No") }),
                    ro(t!("Background transparency").as_ref(), "wo_bg_transparency", if bg_transparency { t!("Yes") } else { t!("No") }),
                ],
            },
            PropSection {
                title: t!("Image Adjust").into_owned(),
                props: vec![
                    ro(t!("Brightness").as_ref(), "wo_brightness", self.brightness.to_string()),
                    ro(t!("Contrast").as_ref(), "wo_contrast", self.contrast.to_string()),
                    ro(t!("Fade").as_ref(), "wo_fade", self.fade.to_string()),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "wo_ox" => self.insertion_point.x = v,
            "wo_oy" => self.insertion_point.y = v,
            "wo_oz" => self.insertion_point.z = v,
            _ => {}
        }
    }
}

impl Transformable for Wipeout {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                reflect_vec3(&mut entity.u_vector.x, &mut entity.u_vector.y, ax, ay, len2);
                reflect_vec3(&mut entity.v_vector.x, &mut entity.v_vector.y, ax, ay, len2);
            }
        });
    }
}
