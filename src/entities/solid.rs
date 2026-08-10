// SOLID entity — 2D filled quadrilateral (or triangle when p3 == p4).
//
// Wireframe: the 4 perimeter edges as RenderObject::Lines.
// Filled:    two triangles on `fill_tris`, preserving the entity's full WCS
//            plane both at top level and through block expansion. The scene
//            keeps a separate 2-D HatchModel only for plot projection; screen
//            rendering filters that flattened copy out.
// Grips:     4 corner grip points.

use acadrust::entities::Solid;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::SnapHint;

fn dvec3(v: &acadrust::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

pub(crate) fn wcs_corners(solid: &Solid) -> [[f64; 3]; 4] {
    let n = (solid.normal.x, solid.normal.y, solid.normal.z);
    let w = |v: &acadrust::types::Vector3| {
        let (x, y, z) = crate::scene::view::transform::ocs_point_to_wcs((v.x, v.y, v.z), n);
        [x, y, z]
    };
    [
        w(&solid.first_corner),
        w(&solid.second_corner),
        w(&solid.third_corner),
        w(&solid.fourth_corner),
    ]
}

/// Return a non-self-intersecting perimeter for either conventional DXF SOLID
/// Z-order or older/perimeter-ordered data. Prefer DXF order when both are
/// valid, but recover legacy solids whose 1-2-4-3 walk forms a bow-tie.
pub(crate) fn perimeter_indices(corners: &[[f64; 3]; 4]) -> [usize; 4] {
    let p = |index: usize| glam::DVec3::from_array(corners[index]);
    let edge = p(1) - p(0);
    let mut normal = edge.cross(p(2) - p(0));
    if normal.length_squared() < 1.0e-20 {
        normal = edge.cross(p(3) - p(0));
    }
    if normal.length_squared() < 1.0e-20 {
        return [0, 1, 3, 2];
    }
    let orient = |a: usize, b: usize, c: usize| {
        (p(b) - p(a)).cross(p(c) - p(a)).dot(normal)
    };
    let segments_cross = |a: usize, b: usize, c: usize, d: usize| {
        let ab_c = orient(a, b, c);
        let ab_d = orient(a, b, d);
        let cd_a = orient(c, d, a);
        let cd_b = orient(c, d, b);
        ((ab_c > 0.0 && ab_d < 0.0) || (ab_c < 0.0 && ab_d > 0.0))
            && ((cd_a > 0.0 && cd_b < 0.0) || (cd_a < 0.0 && cd_b > 0.0))
    };
    let order_crosses = |order: [usize; 4]| {
        segments_cross(order[0], order[1], order[2], order[3])
            || segments_cross(order[1], order[2], order[3], order[0])
    };
    let dxf = [0, 1, 3, 2];
    let perimeter = [0, 1, 2, 3];
    if order_crosses(dxf) && !order_crosses(perimeter) {
        perimeter
    } else {
        dxf
    }
}

impl RenderConvertible for Solid {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        // SOLID corners are OCS. Map them to WCS, then resolve either the DXF
        // Z-order or legacy perimeter order before building edges and fill.
        let corners = wcs_corners(self);
        let order = perimeter_indices(&corners);
        let [p0, p1, p2, p3] = order.map(|index| corners[index]);
        let pts = vec![
            p0,
            p1,
            [f64::NAN; 3],
            p1,
            p2,
            [f64::NAN; 3],
            p2,
            p3,
            [f64::NAN; 3],
            p3,
            p0,
        ];

        let dvp = |p: [f64; 3]| glam::DVec3::from_array(p);
        let snap = corners
            .iter()
            .copied()
            .map(|point| (dvp(point), SnapHint::Node))
            .collect();

        // Fill the resolved perimeter as two triangles. For a triangle the last
        // two points coincide and the second triangle degenerates harmlessly.
        let fill_tris = vec![p0, p1, p2, p0, p2, p3];

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts),
            snap_pts: snap,
            tangent_geoms: vec![],
            key_vertices: corners.to_vec(),
            fill_tris,
        })
    }
}

impl Grippable for Solid {
    fn grips(&self) -> Vec<GripDef> {
        vec![
            square_grip(0, dvec3(&self.first_corner)),
            square_grip(1, dvec3(&self.second_corner)),
            square_grip(2, dvec3(&self.third_corner)),
            square_grip(3, dvec3(&self.fourth_corner)),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let corner = match grip_id {
            0 => &mut self.first_corner,
            1 => &mut self.second_corner,
            2 => &mut self.third_corner,
            3 => &mut self.fourth_corner,
            _ => return,
        };
        match apply {
            GripApply::Translate(d) => {
                corner.x += d.x as f64;
                corner.y += d.y as f64;
                corner.z += d.z as f64;
            }
            GripApply::Absolute(p) => {
                corner.x = p.x as f64;
                corner.y = p.y as f64;
                corner.z = p.z as f64;
            }
        }
    }
}

impl PropertyEditable for Solid {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        // Elevation is the OCS Z shared by the planar corners (no dedicated
        // field on the entity); reported from the first corner's Z.
        let elevation = self.first_corner.z;
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Point 1 X").as_ref(), "sl_p1x", self.first_corner.x),
                edit(t!("Point 1 Y").as_ref(), "sl_p1y", self.first_corner.y),
                edit(t!("Point 1 Z").as_ref(), "sl_p1z", self.first_corner.z),
                edit(t!("Point 2 X").as_ref(), "sl_p2x", self.second_corner.x),
                edit(t!("Point 2 Y").as_ref(), "sl_p2y", self.second_corner.y),
                edit(t!("Point 2 Z").as_ref(), "sl_p2z", self.second_corner.z),
                edit(t!("Point 3 X").as_ref(), "sl_p3x", self.third_corner.x),
                edit(t!("Point 3 Y").as_ref(), "sl_p3y", self.third_corner.y),
                edit(t!("Point 3 Z").as_ref(), "sl_p3z", self.third_corner.z),
                edit(t!("Point 4 X").as_ref(), "sl_p4x", self.fourth_corner.x),
                edit(t!("Point 4 Y").as_ref(), "sl_p4y", self.fourth_corner.y),
                edit(t!("Point 4 Z").as_ref(), "sl_p4z", self.fourth_corner.z),
                ro(t!("Elevation").as_ref(), "sl_elev", format!("{:.4}", elevation)),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "sl_p1x" => self.first_corner.x = v,
            "sl_p1y" => self.first_corner.y = v,
            "sl_p1z" => self.first_corner.z = v,
            "sl_p2x" => self.second_corner.x = v,
            "sl_p2y" => self.second_corner.y = v,
            "sl_p2z" => self.second_corner.z = v,
            "sl_p3x" => self.third_corner.x = v,
            "sl_p3y" => self.third_corner.y = v,
            "sl_p3z" => self.third_corner.z = v,
            "sl_p4x" => self.fourth_corner.x = v,
            "sl_p4y" => self.fourth_corner.y = v,
            "sl_p4z" => self.fourth_corner.z = v,
            _ => {}
        }
    }
}

impl Transformable for Solid {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(
            self,
            t,
            |entity, p1, p2| {
                for corner in [
                    &mut entity.first_corner,
                    &mut entity.second_corner,
                    &mut entity.third_corner,
                    &mut entity.fourth_corner,
                ] {
                    crate::scene::view::transform::reflect_xy_point(
                        &mut corner.x,
                        &mut corner.y,
                        p1,
                        p2,
                    );
                }
            },
        );
    }
}
