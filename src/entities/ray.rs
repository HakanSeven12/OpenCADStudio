use crate::t;
use document::entities::{Ray, XLine};

use crate::entities::common::{edit_prop as edit, format_length, ro_prop as ro, square_grip};
use crate::entities::curve::{point_along, unit_direction};
use crate::entities::traits::{Grippable, PropertyEditable, RenderConvertible, Transformable};
use crate::entities::transform::EntityTransform;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};

/// Display length used when rendering semi-infinite / infinite lines.
const DISPLAY_EXTENT: f64 = 1_000_000.0;

/// Points along `dir` from `bp`, one per decade (1, 10, ... DISPLAY_EXTENT),
/// in the order of `sign` (+1 outward, -1 inward towards `bp`). A single
/// segment to the far end degenerates on screen when the view is zoomed in
/// far: the GPU expands it in f32 from ends ~1e6 units away, its sub-pixel
/// width offset rounds away and the line vanishes. With a vertex every decade
/// the segment under the view ends within ~10x the view's distance from the
/// base point.
fn decade_points(bp: [f64; 3], dir: [f64; 3], sign: f64) -> Vec<[f64; 3]> {
    let mut steps = Vec::new();
    let mut d = 1.0;
    while d <= DISPLAY_EXTENT {
        steps.push(d);
        d *= 10.0;
    }
    if sign < 0.0 {
        steps.reverse();
    }
    steps
        .into_iter()
        .map(|d| {
            let t = sign * d;
            [bp[0] + dir[0] * t, bp[1] + dir[1] * t, bp[2] + dir[2] * t]
        })
        .collect()
}

// ── Ray (semi-infinite line) ──────────────────────────────────────────────────

impl RenderConvertible for Ray {
    fn to_render(&self, _document: &document::Document) -> Option<RenderEntity> {
        let bp = self.base_point;
        let dir = self.direction;
        // Normalize direction to avoid f32 overflow when DXF stores
        // unnormalized direction vectors (garbage data in some exporters).
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        if len < 1e-10 {
            return None;
        }
        let (nx, ny, nz) = (dir.x / len, dir.y / len, dir.z / len);
        let start: [f64; 3] = [bp.x, bp.y, bp.z];
        let mut run = vec![start];
        run.extend(decade_points(start, [nx, ny, nz], 1.0));
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(run),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![start],
            fill_tris: vec![],
        })
    }
}

impl Grippable for Ray {
    fn grips(&self) -> Vec<GripDef> {
        let bp = &self.base_point;
        let dir = &self.direction;
        // Grip 0: base point (movable)
        // Grip 1: a point along the direction (changes direction). A stretch
        // grip, not a move handle: `center_grip` would flag it `is_midpoint`
        // and the drag would arrive as `Translate`, which the direction arm
        // ignores — the grip would silently do nothing.
        let guide_dist = 10.0_f64;
        vec![
            square_grip(0, glam::DVec3::new(bp.x, bp.y, bp.z)),
            square_grip(
                1,
                glam::DVec3::new(
                    bp.x + dir.x * guide_dist,
                    bp.y + dir.y * guide_dist,
                    bp.z + dir.z * guide_dist,
                ),
            ),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        match (grip_id, apply) {
            (0, GripApply::Translate(d)) => {
                self.base_point.x += d.x;
                self.base_point.y += d.y;
                self.base_point.z += d.z;
            }
            (0, GripApply::Absolute(p)) => {
                self.base_point.x = p.x;
                self.base_point.y = p.y;
                self.base_point.z = p.z;
            }
            (1, GripApply::Absolute(p)) => {
                // New direction = grip point - base point, normalized.
                let dx = p.x - self.base_point.x;
                let dy = p.y - self.base_point.y;
                let dz = p.z - self.base_point.z;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len > 1e-9 {
                    self.direction.x = dx / len;
                    self.direction.y = dy / len;
                    self.direction.z = dz / len;
                }
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Ray {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Start X").as_ref(), "ray_bx", self.base_point.x),
                edit(t!("Start Y").as_ref(), "ray_by", self.base_point.y),
                edit(t!("Start Z").as_ref(), "ray_bz", self.base_point.z),
                edit(
                    t!("Direction vector X").as_ref(),
                    "ray_dx",
                    self.direction.x,
                ),
                edit(
                    t!("Direction vector Y").as_ref(),
                    "ray_dy",
                    self.direction.y,
                ),
                edit(
                    t!("Direction vector Z").as_ref(),
                    "ray_dz",
                    self.direction.z,
                ),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "ray_bx" => self.base_point.x = v,
            "ray_by" => self.base_point.y = v,
            "ray_bz" => self.base_point.z = v,
            "ray_dx" => {
                self.direction.x = v;
            }
            "ray_dy" => {
                self.direction.y = v;
            }
            "ray_dz" => {
                self.direction.z = v;
            }
            _ => {}
        }
    }
}

impl Transformable for Ray {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(
            self,
            t,
            |entity, p1, p2| {
                crate::scene::view::transform::reflect_xy_point(
                    &mut entity.base_point.x,
                    &mut entity.base_point.y,
                    p1,
                    p2,
                );
                // Mirror the direction: negate the component perpendicular to mirror axis.
                let ax = p2.x - p1.x;
                let ay = p2.y - p1.y;
                let len2 = ax * ax + ay * ay;
                if len2 > 1e-12 {
                    let d = &mut entity.direction;
                    let dot = d.x * ax + d.y * ay;
                    d.x = 2.0 * dot * ax / len2 - d.x;
                    d.y = 2.0 * dot * ay / len2 - d.y;
                }
            },
        );
    }
}

// ── XLine (construction line, infinite) ──────────────────────────────────────

impl RenderConvertible for XLine {
    fn to_render(&self, _document: &document::Document) -> Option<RenderEntity> {
        let bp = self.base_point;
        let dir = self.direction;
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        if len < 1e-10 {
            return None;
        }
        let (nx, ny, nz) = (dir.x / len, dir.y / len, dir.z / len);
        let base: [f64; 3] = [bp.x, bp.y, bp.z];
        let unit = [nx, ny, nz];
        let mut run = decade_points(base, unit, -1.0);
        run.push(base);
        run.extend(decade_points(base, unit, 1.0));
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(run),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![[bp.x, bp.y, bp.z]],
            fill_tris: vec![],
        })
    }
}

impl Grippable for XLine {
    fn grips(&self) -> Vec<GripDef> {
        let bp = &self.base_point;
        let dir = &self.direction;
        let guide_dist = 10.0_f64;
        vec![
            square_grip(0, glam::DVec3::new(bp.x, bp.y, bp.z)),
            // Direction handle: a stretch grip, not a move handle (see Ray).
            square_grip(
                1,
                glam::DVec3::new(
                    bp.x + dir.x * guide_dist,
                    bp.y + dir.y * guide_dist,
                    bp.z + dir.z * guide_dist,
                ),
            ),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        match (grip_id, apply) {
            (0, GripApply::Translate(d)) => {
                self.base_point.x += d.x;
                self.base_point.y += d.y;
                self.base_point.z += d.z;
            }
            (0, GripApply::Absolute(p)) => {
                self.base_point.x = p.x;
                self.base_point.y = p.y;
                self.base_point.z = p.z;
            }
            (1, GripApply::Absolute(p)) => {
                let dx = p.x - self.base_point.x;
                let dy = p.y - self.base_point.y;
                let dz = p.z - self.base_point.z;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len > 1e-9 {
                    self.direction.x = dx / len;
                    self.direction.y = dy / len;
                    self.direction.z = dz / len;
                }
            }
            _ => {}
        }
    }
}

impl PropertyEditable for XLine {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let second_point = point_along(self.base_point, self.direction, 1.0);
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Base X").as_ref(), "xl_bx", self.base_point.x),
                edit(t!("Base Y").as_ref(), "xl_by", self.base_point.y),
                edit(t!("Base Z").as_ref(), "xl_bz", self.base_point.z),
                edit(t!("Second X").as_ref(), "xl_sx", second_point.x),
                edit(t!("Second Y").as_ref(), "xl_sy", second_point.y),
                edit(t!("Second Z").as_ref(), "xl_sz", second_point.z),
                ro(
                    t!("Direction vector X").as_ref(),
                    "xl_dx",
                    format_length(self.direction.x),
                ),
                ro(
                    t!("Direction vector Y").as_ref(),
                    "xl_dy",
                    format_length(self.direction.y),
                ),
                ro(
                    t!("Direction vector Z").as_ref(),
                    "xl_dz",
                    format_length(self.direction.z),
                ),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "xl_bx" => self.base_point.x = v,
            "xl_by" => self.base_point.y = v,
            "xl_bz" => self.base_point.z = v,
            "xl_sx" | "xl_sy" | "xl_sz" => {
                let mut second_point = self.point_at(1.0);
                match field {
                    "xl_sx" => second_point.x = v,
                    "xl_sy" => second_point.y = v,
                    "xl_sz" => second_point.z = v,
                    _ => unreachable!(),
                }
                if let Some(direction) = unit_direction(self.base_point, second_point) {
                    self.direction = direction;
                }
            }
            _ => {}
        }
    }
}

impl Transformable for XLine {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(
            self,
            t,
            |entity, p1, p2| {
                crate::scene::view::transform::reflect_xy_point(
                    &mut entity.base_point.x,
                    &mut entity.base_point.y,
                    p1,
                    p2,
                );
                let ax = p2.x - p1.x;
                let ay = p2.y - p1.y;
                let len2 = ax * ax + ay * ay;
                if len2 > 1e-12 {
                    let d = &mut entity.direction;
                    let dot = d.x * ax + d.y * ay;
                    d.x = 2.0 * dot * ax / len2 - d.x;
                    d.y = 2.0 * dot * ay / len2 - d.y;
                }
            },
        );
    }
}

#[cfg(test)]
mod display_run_tests {
    use super::*;
    use document::types::Vector3;

    fn run(entity: Option<RenderEntity>) -> Vec<[f64; 3]> {
        match entity.expect("renders").object {
            RenderObject::Lines(points) => points,
            _ => panic!("expected a line run"),
        }
    }

    #[test]
    fn xline_renders_a_vertex_every_decade_both_ways() {
        let doc = document::Document::new();
        let xl = XLine::new(Vector3::new(700.0, 20.0, 0.0), Vector3::new(2.0, 0.0, 0.0));
        let pts = run(xl.to_render(&doc));
        assert_eq!(pts.len(), 15);
        assert_eq!(pts[7], [700.0, 20.0, 0.0]);
        assert_eq!(pts[0][0], 700.0 - DISPLAY_EXTENT);
        assert_eq!(pts[6][0], 699.0);
        assert_eq!(pts[8][0], 701.0);
        assert_eq!(pts[14][0], 700.0 + DISPLAY_EXTENT);
        assert!(pts.windows(2).all(|w| w[1][0] > w[0][0]));
    }

    #[test]
    fn ray_renders_from_its_base_with_decade_vertices() {
        let doc = document::Document::new();
        let ray = Ray::new(Vector3::new(1.0, 2.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
        let pts = run(ray.to_render(&doc));
        assert_eq!(pts.len(), 8);
        assert_eq!(pts[0], [1.0, 2.0, 0.0]);
        assert_eq!(pts[1], [1.0, 1.0, 0.0]);
        assert_eq!(pts[7], [1.0, 2.0 - DISPLAY_EXTENT, 0.0]);
    }
}
