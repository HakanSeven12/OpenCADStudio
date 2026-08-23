// Interactive kernel primitives stored as ACIS Solid3D entities.

use acadrust::entities::Solid3D;
use acadrust::objects::SolidHistoryOperation;
use acadrust::EntityType;
use cadkernel::brep::Body;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::solid_model;
use crate::scene::model::wire_model::WireModel;
use crate::t;

/// Which primitive a `PrimitiveCommand` builds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Box,
    Wedge,
    Cylinder,
    Cone,
    Sphere,
    Pyramid,
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
            "PYRAMID" | "PYR" => Shape::Pyramid,
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
            Shape::Pyramid => "PYRAMID",
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

    fn cursor_height(&self, point: DVec3) -> f64 {
        (self.plane.to_local(point).z - self.pts[0].z)
            .max(1e-6)
    }

    fn place_preview(&self, mut preview: WireModel) -> WireModel {
        for point in &mut preview.points {
            if !point[0].is_nan() {
                *point = self
                    .plane
                    .to_world(glam::Vec3::from_array(*point).as_dvec3())
                    .as_vec3()
                    .to_array();
            }
        }
        preview
    }

    fn history_transform(&self, origin: DVec3) -> [f64; 16] {
        glam::DMat4::from_cols(
            self.plane.x.extend(0.0),
            self.plane.y.extend(0.0),
            self.plane.z.extend(0.0),
            self.plane.to_world(origin).extend(1.0),
        )
        .to_cols_array()
    }

    /// Build the persistent kernel body and its history node.
    fn build(&self, height: f64) -> Option<(Body, SolidHistoryOperation)> {
        use crate::scene::model::solid_history;

        let (solid, history) = match self.shape {
            Shape::Box | Shape::Wedge => {
                let (a, b) = (self.pts[0], self.pts[1]);
                let length = (b.x - a.x).abs();
                let width = (b.y - a.y).abs();
                if length < 1e-6 || width < 1e-6 || height < 1e-6 {
                    return None;
                }
                if self.shape == Shape::Box {
                    let origin = DVec3::new(a.x.min(b.x), a.y.min(b.y), a.z);
                    let center = [
                        (a.x + b.x) / 2.0,
                        (a.y + b.y) / 2.0,
                        a.z + height / 2.0,
                    ];
                    (
                        solid_model::box_solid(center, length, width, height),
                        solid_history::box_op(
                            self.history_transform(origin),
                            length,
                            width,
                            height,
                        ),
                    )
                } else {
                    let origin = [a.x.min(b.x), a.y.min(b.y), a.z];
                    (
                        solid_model::wedge_solid(origin, length, width, height),
                        solid_history::wedge_op(
                            self.history_transform(DVec3::from_array(origin)),
                            length,
                            width,
                            height,
                        ),
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
                        solid_model::cylinder_solid(center, r, height),
                        solid_history::cylinder_op(
                            self.history_transform(c),
                            r,
                            height,
                        ),
                    )
                } else {
                    (
                        solid_model::cone_solid(center, r, height),
                        solid_history::cone_op(
                            self.history_transform(c),
                            r,
                            height,
                        ),
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
                    solid_model::sphere_solid(center, r),
                    solid_history::sphere_op(self.history_transform(c), r),
                )
            }
            Shape::Pyramid => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 || height < 1e-6 {
                    return None;
                }
                (
                    solid_model::pyramid_solid([c.x, c.y, c.z], r, height, 4),
                    solid_history::pyramid_op(self.history_transform(c), r, height, 4),
                )
            }
            Shape::Torus => {
                let c = self.pts[0];
                let first = (self.pts[1] - c).length();
                let second = (self.pts[2] - c).length();
                let outer = first.max(second);
                let inner = first.min(second);
                if inner < 1e-6 || outer - inner < 1e-6 {
                    return None;
                }
                let major = (outer + inner) * 0.5;
                let minor = (outer - inner) * 0.5;
                let center = [c.x, c.y, c.z];
                (
                    solid_model::torus_solid(center, major, minor),
                    solid_history::torus_op(
                        self.history_transform(c),
                        major,
                        minor,
                    ),
                )
            }
        };
        let solid = solid?;
        Some((solid, history))
    }

    fn commit(&self, height: f64) -> CmdResult {
        match self.build(height) {
            Some((solid, history)) => {
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
                    Some(placed) => {
                        let Some(document) =
                            crate::scene::convert::acis_export::solid_to_sat(&placed)
                        else {
                            return CmdResult::Cancel;
                        };
                        let mut entity = Solid3D::new();
                        entity.set_sat_document(&document);
                        entity.wires = solid_model::edge_wires(&placed);
                        CmdResult::CommitSolid {
                            entity: EntityType::Solid3D(entity),
                            solid: Box::new(placed),
                            history,
                        }
                    }
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

    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        self.height_step.then(|| {
            (
                self.plane.to_world(self.pts[0]),
                self.plane.z.normalize_or_zero(),
            )
        })
    }

    fn name(&self) -> &'static str {
        self.shape.name()
    }

    fn prompt(&self) -> String {
        let n = self.shape.name();
        if self.height_step {
            return t!("%{n}  Specify height <Enter for default>:", n = n).into_owned();
        }
        match (self.shape, self.pts.len()) {
            (Shape::Torus, 0) => t!("%{n}  Specify center point:", n = n).into_owned(),
            (Shape::Torus, 1) => t!("%{n}  Specify outer radius:", n = n).into_owned(),
            (Shape::Torus, _) => t!("%{n}  Specify inner radius:", n = n).into_owned(),
            (shape, 0) if shape.radial() => {
                t!("%{n}  Specify center point:", n = n).into_owned()
            }
            (shape, _) if shape.radial() => {
                t!("%{n}  Specify radius:", n = n).into_owned()
            }
            (_, 0) => t!("%{n}  Specify first corner:", n = n).into_owned(),
            (_, _) => t!("%{n}  Specify opposite corner:", n = n).into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.height_step {
            return self.commit(self.cursor_height(pt));
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
        if self.pts.is_empty() {
            return None;
        }
        if self.height_step {
            return Some(self.place_preview(height_wire(
                self.shape,
                &self.pts,
                self.cursor_height(pt),
            )));
        }
        let mut foot = self.pts.clone();
        foot.push(self.plane.to_local(pt));
        Some(self.place_preview(footprint_wire(self.shape, &foot)))
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};

        self.height_step.then(|| DynSpec {
            anchor: DynAnchor::Point(self.plane.to_world(self.pts[0])),
            fields: vec![DynFieldSpec::new(DynRole::Height)],
            guide: DynGuide::None,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        self.height_step.then(|| self.cursor_height(cursor))
    }
}

// ── Footprint preview ───────────────────────────────────────────────────────

fn footprint_wire(shape: Shape, pts: &[DVec3]) -> WireModel {
    let mut points: Vec<[f32; 3]> = Vec::new();
    if shape == Shape::Pyramid {
        let center = pts[0];
        let radius = (pts[1] - center).length();
        let corners: [DVec3; 4] = std::array::from_fn(|index| {
            let angle = index as f64 * std::f64::consts::FRAC_PI_2;
            center + DVec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
        });
        push_loop(&mut points, &corners);
    } else if shape.radial() {
        let c = pts[0];
        let r = (pts[1] - c).length();
        circle_points(&mut points, c, r);
        if shape == Shape::Torus && pts.len() >= 3 {
            let inner = (pts[2] - c).length();
            points.push([f32::NAN; 3]);
            circle_points(&mut points, c, inner);
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

fn height_wire(shape: Shape, pts: &[DVec3], height: f64) -> WireModel {
    let mut points = Vec::new();
    match shape {
        Shape::Box => {
            let (a, b) = (pts[0], pts[1]);
            let base = [
                DVec3::new(a.x, a.y, a.z),
                DVec3::new(b.x, a.y, a.z),
                DVec3::new(b.x, b.y, a.z),
                DVec3::new(a.x, b.y, a.z),
            ];
            let top = base.map(|point| point + DVec3::Z * height);
            push_loop(&mut points, &base);
            push_loop(&mut points, &top);
            for i in 0..4 {
                push_segment(&mut points, base[i], top[i]);
            }
        }
        Shape::Wedge => {
            let (a, b) = (pts[0], pts[1]);
            let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
            let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
            let low = [
                DVec3::new(x0, y0, a.z),
                DVec3::new(x1, y0, a.z),
                DVec3::new(x0, y0, a.z + height),
            ];
            let high = low.map(|point| DVec3::new(point.x, y1, point.z));
            push_loop(&mut points, &low);
            push_loop(&mut points, &high);
            for i in 0..3 {
                push_segment(&mut points, low[i], high[i]);
            }
        }
        Shape::Cylinder | Shape::Cone | Shape::Pyramid => {
            let center = pts[0];
            let radius = (pts[1] - center).length();
            if shape == Shape::Pyramid {
                let base: [DVec3; 4] = std::array::from_fn(|index| {
                    let angle = index as f64 * std::f64::consts::FRAC_PI_2;
                    center + DVec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
                });
                push_loop(&mut points, &base);
                let apex = center + DVec3::Z * height;
                for corner in base {
                    push_segment(&mut points, corner, apex);
                }
                return wire("primitive_height_preview", points);
            }
            push_circle(&mut points, center, radius);
            if shape == Shape::Cylinder {
                push_circle(&mut points, center + DVec3::Z * height, radius);
                for i in 0..4 {
                    let angle = i as f64 * std::f64::consts::FRAC_PI_2;
                    let base = center + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0);
                    push_segment(&mut points, base, base + DVec3::Z * height);
                }
            } else {
                let apex = center + DVec3::Z * height;
                for i in 0..4 {
                    let angle = i as f64 * std::f64::consts::FRAC_PI_2;
                    let base = center + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0);
                    push_segment(&mut points, base, apex);
                }
            }
        }
        Shape::Sphere | Shape::Torus => {}
    }
    wire("primitive_height_preview", points)
}

fn push_break(points: &mut Vec<[f32; 3]>) {
    if !points.is_empty() {
        points.push([f32::NAN; 3]);
    }
}

fn push_loop<const N: usize>(points: &mut Vec<[f32; 3]>, path: &[DVec3; N]) {
    push_break(points);
    points.extend(path.iter().chain(path.first()).map(|point| point.as_vec3().to_array()));
}

fn push_segment(points: &mut Vec<[f32; 3]>, a: DVec3, b: DVec3) {
    push_break(points);
    points.extend([a.as_vec3().to_array(), b.as_vec3().to_array()]);
}

fn push_circle(points: &mut Vec<[f32; 3]>, center: DVec3, radius: f64) {
    push_break(points);
    circle_points(points, center, radius);
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
    names: &["BOX", "WEDGE", "CYLINDER", "CONE", "SPHERE", "PYRAMID", "PYR", "TORUS"]
});

fn wire(name: &str, points: Vec<[f32; 3]>) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
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
