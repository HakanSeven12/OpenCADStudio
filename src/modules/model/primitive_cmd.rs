// 3D primitive creation — BOX / CYLINDER / CONE / SPHERE / WEDGE / PYRAMID /
// TORUS. Each is placed CAD-style with a few clicks (planar footprint first,
// then a height value), then built as a real ACIS `Solid3D` via acadrust's
// `acis::primitives` builders. `Scene::add_entity` tessellates the SAT B-rep
// into the 3D mesh pipeline, so the solid renders, selects, and saves to DXF.
//
// A matching the kernel `Solid` is cached on the scene (see model/mod.rs) when the
// entity is committed, so the Design-group boolean tools can combine it.

use acadrust::entities::Solid3D;
use acadrust::{primitives, EntityType};
use glam::DVec3;
use crate::t;
use acadrust::kernel::brep::Body;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::solid_model;
use crate::scene::model::wire_model::WireModel;

/// Which primitive a `PrimitiveCommand` builds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Box,
    Wedge,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

impl Shape {
    fn from_id(id: &str) -> Option<Shape> {
        Some(match id {
            "BOX" => Shape::Box,
            "WEDGE" => Shape::Wedge,
            "CYLINDER" => Shape::Cylinder,
            "CONE" => Shape::Cone,
            "SPHERE" => Shape::Sphere,
            "TORUS" => Shape::Torus,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Shape::Box => "BOX",
            Shape::Wedge => "WEDGE",
            Shape::Cylinder => "CYLINDER",
            Shape::Cone => "CONE",
            Shape::Sphere => "SPHERE",
            Shape::Torus => "TORUS",
        }
    }
    /// True for footprints picked as a centre + radius (round shapes); false
    /// for corner-to-corner footprints (box/wedge).
    fn radial(self) -> bool {
        !matches!(self, Shape::Box | Shape::Wedge)
    }
    /// Whether a height value is collected after the footprint.
    fn needs_height(self) -> bool {
        !matches!(self, Shape::Sphere | Shape::Torus)
    }
}

pub struct PrimitiveCommand {
    shape: Shape,
    /// Footprint points collected so far (local/world XY, z = 0).
    pts: Vec<DVec3>,
    /// True once the footprint is set and we are collecting the height.
    height_step: bool,
    plane: WorkingPlane,
}

impl PrimitiveCommand {
    pub fn new(id: &str) -> Self {
        Self {
            shape: Shape::from_id(id).unwrap_or(Shape::Box),
            pts: Vec::new(),
            height_step: false,
            plane: WorkingPlane::default(),
        }
    }

    /// Number of footprint points the shape needs before the height step.
    fn footprint_pts(&self) -> usize {
        match self.shape {
            Shape::Torus => 3, // centre, major-radius, minor-radius
            _ => 2,            // corner/corner  or  centre/radius
        }
    }

    /// A reasonable default height when the user just presses Enter.
    fn default_height(&self) -> f64 {
        match self.shape {
            Shape::Box | Shape::Wedge => {
                let d = self.pts[1] - self.pts[0];
                d.x.abs().max(d.y.abs())
            }
            _ => (self.pts[1] - self.pts[0]).length(),
        }
        .max(1.0)
    }

    /// Build both the acadrust `Solid3D` (ACIS, for persistence) and the
    /// kernel `Body` (rendering + booleans) from the footprint + `height`.
    fn build(&self, height: f64) -> Option<(EntityType, Body)> {
        let (doc, solid) = match self.shape {
            Shape::Box | Shape::Wedge => {
                let (a, b) = (self.pts[0], self.pts[1]);
                let length = (b.x - a.x).abs();
                let width = (b.y - a.y).abs();
                if length < 1e-6 || width < 1e-6 || height < 1e-6 {
                    return None;
                }
                if self.shape == Shape::Box {
                    let center = [
                        (a.x + b.x) / 2.0,
                        (a.y + b.y) / 2.0,
                        a.z + height / 2.0,
                    ];
                    (
                        primitives::build_box(center, length, width, height),
                        solid_model::box_solid(center, length, width, height),
                    )
                } else {
                    let origin = [a.x.min(b.x), a.y.min(b.y), a.z];
                    (
                        primitives::build_wedge(origin, length, width, height),
                        solid_model::wedge_solid(origin, length, width, height),
                    )
                }
            }
            Shape::Cylinder | Shape::Cone => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 || height < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                if self.shape == Shape::Cylinder {
                    (
                        primitives::build_cylinder(center, r, height),
                        solid_model::cylinder_solid(center, r, height),
                    )
                } else {
                    (
                        primitives::build_cone(center, r, height),
                        solid_model::cone_solid(center, r, height),
                    )
                }
            }
            Shape::Sphere => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                (
                    primitives::build_sphere(center, r),
                    solid_model::sphere_solid(center, r),
                )
            }
            Shape::Torus => {
                let c = self.pts[0];
                let major = (self.pts[1] - c).length();
                let minor = (self.pts[2] - self.pts[1]).length();
                if major < 1e-6 || minor < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                (
                    primitives::build_torus(center, major, minor),
                    solid_model::torus_solid(center, major, minor),
                )
            }
        };
        let solid = solid?;
        let mut s3d = Solid3D::new();
        s3d.set_sat_document(&doc);
        // Edge wires make the solid click-pickable and draw a wireframe over
        // the shaded mesh.
        s3d.wires = solid_model::edge_wires(&solid);
        Some((EntityType::Solid3D(s3d), solid))
    }

    fn commit(&self, height: f64) -> CmdResult {
        match self.build(height) {
            Some((entity, solid)) => {
                // Built upright in its own frame, then put on the working
                // plane — the same move `place_entity` makes for the ACIS
                // copy, so the two stay on top of each other.
                let placed = solid_model::placed(
                    &solid,
                    [self.plane.x.x, self.plane.x.y, self.plane.x.z],
                    [self.plane.y.x, self.plane.y.y, self.plane.y.z],
                    [self.plane.z.x, self.plane.z.y, self.plane.z.z],
                    [
                        self.plane.origin.x,
                        self.plane.origin.y,
                        self.plane.origin.z,
                    ],
                );
                match placed {
                    Some(placed) => CmdResult::CommitSolid {
                        entity: self.plane.place_entity(entity),
                        solid: Box::new(placed),
                    },
                    None => CmdResult::Cancel,
                }
            }
            None => CmdResult::Cancel,
        }
    }
}

impl CadCommand for PrimitiveCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        self.shape.name()
    }

    fn prompt(&self) -> String {
        let n = self.shape.name();
        if self.height_step {
            return t!("%{n}  Specify height <Enter for default>:", n = n).into_owned();
        }
        match (self.shape.radial(), self.pts.len()) {
            (false, 0) => t!("%{n}  Specify first corner:", n = n).into_owned(),
            (false, _) => t!("%{n}  Specify opposite corner:", n = n).into_owned(),
            (true, 0) => t!("%{n}  Specify center point:", n = n).into_owned(),
            (true, 1) => t!("%{n}  Specify radius:", n = n).into_owned(),
            (true, _) => t!("%{n}  Specify tube radius:", n = n).into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.height_step {
            // A click in the ground plane has no Z; use its distance from the
            // footprint centre as the height magnitude.
            let h = (self.plane.to_local(pt) - self.pts[0]).length();
            return self.commit(h.max(1e-6));
        }
        self.pts.push(self.plane.to_local(pt));
        if self.pts.len() < self.footprint_pts() {
            return CmdResult::NeedPoint;
        }
        // Footprint complete.
        if self.shape.needs_height() {
            self.height_step = true;
            CmdResult::NeedPoint
        } else {
            self.commit(0.0)
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.height_step {
            let h = self.default_height();
            return self.commit(h);
        }
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        self.height_step
    }

    fn on_text_input(&mut self, raw: &str) -> Option<CmdResult> {
        if !self.height_step {
            return None;
        }
        let h: f64 = raw.trim().parse().ok().filter(|v| *v > 0.0)?;
        Some(self.commit(h))
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.height_step || self.pts.is_empty() {
            return None;
        }
        let mut foot = self.pts.clone();
        foot.push(self.plane.to_local(pt));
        let mut preview = footprint_wire(self.shape, &foot);
        preview.points = preview
            .points
            .iter()
            .map(|point| {
                if point[0].is_nan() {
                    *point
                } else {
                    self.plane
                        .to_world(glam::Vec3::from_array(*point).as_dvec3())
                        .as_vec3()
                        .to_array()
                }
            })
            .collect();
        Some(preview)
    }
}

// ── Footprint preview ───────────────────────────────────────────────────────

fn footprint_wire(shape: Shape, pts: &[DVec3]) -> WireModel {
    let mut points: Vec<[f32; 3]> = Vec::new();
    if shape.radial() {
        let c = pts[0];
        let r = (pts[1] - c).length();
        circle_points(&mut points, c, r);
        if shape == Shape::Torus && pts.len() >= 3 {
            // outer ring at major + minor for a quick torus hint
            let minor = (pts[2] - pts[1]).length();
            points.push([f32::NAN; 3]);
            circle_points(&mut points, c, r + minor);
        }
    } else {
        let (a, b) = (pts[0], pts[1]);
        points.extend_from_slice(&[
            [a.x as f32, a.y as f32, a.z as f32],
            [b.x as f32, a.y as f32, a.z as f32],
            [b.x as f32, b.y as f32, a.z as f32],
            [a.x as f32, b.y as f32, a.z as f32],
            [a.x as f32, a.y as f32, a.z as f32],
        ]);
    }
    wire("primitive_preview", points)
}

fn circle_points(out: &mut Vec<[f32; 3]>, c: DVec3, r: f64) {
    const SEG: usize = 48;
    for i in 0..=SEG {
        let t = i as f64 / SEG as f64 * std::f64::consts::TAU;
        out.push([
            (c.x + r * t.cos()) as f32,
            (c.y + r * t.sin()) as f32,
            c.z as f32,
        ]);
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["BOX", "WEDGE", "CYLINDER", "CONE", "SPHERE", "TORUS"]
});

fn wire(name: &str, points: Vec<[f32; 3]>) -> WireModel {
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
        name: name.into(),
        points,
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        aci: 0,
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}
