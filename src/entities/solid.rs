// SOLID geometry comes from the kernel; entity coordinates stay in WCS here.

use acadrust::entities::Solid;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::SnapHint;

type Edge = ([f64; 3], [f64; 3]);

struct SolidGeometry {
    edges: Vec<Edge>,
    fill_tris: Vec<[f64; 3]>,
    extruded: bool,
}

fn normal_tuple(solid: &Solid) -> (f64, f64, f64) {
    let normal = glam::DVec3::new(solid.normal.x, solid.normal.y, solid.normal.z)
        .normalize_or(glam::DVec3::Z);
    (normal.x, normal.y, normal.z)
}

fn dvec3(v: [f64; 3]) -> glam::DVec3 {
    glam::DVec3::from_array(v)
}

pub(crate) fn wcs_corners(solid: &Solid) -> [[f64; 3]; 4] {
    let n = normal_tuple(solid);
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

fn set_wcs_corner(solid: &mut Solid, index: usize, point: glam::DVec3) {
    let n = normal_tuple(solid);
    let (x, y, z) = crate::scene::view::transform::wcs_point_to_ocs(
        (point.x, point.y, point.z),
        n,
    );
    let corner = match index {
        0 => &mut solid.first_corner,
        1 => &mut solid.second_corner,
        2 => &mut solid.third_corner,
        3 => &mut solid.fourth_corner,
        _ => return,
    };
    corner.x = x;
    corner.y = y;
    corner.z = z;
}

fn push_edge(lines: &mut Vec<[f64; 3]>, start: [f64; 3], end: [f64; 3]) {
    lines.extend([start, end, [f64::NAN; 3]]);
}

fn boundary(solid: &Solid) -> Vec<[f64; 3]> {
    let corners = wcs_corners(solid);
    if solid.is_triangle() {
        vec![corners[0], corners[1], corners[2]]
    } else {
        vec![corners[0], corners[1], corners[3], corners[2]]
    }
}

fn boundary_edges(points: &[[f64; 3]]) -> Vec<Edge> {
    (0..points.len())
        .map(|index| (points[index], points[(index + 1) % points.len()]))
        .collect()
}

fn kernel_tolerance(points: &[[f64; 3]]) -> f64 {
    cadkernel::space::coplanarity_tolerance(points).max(1.0e-12)
}

fn extrusion_body(
    solid: &Solid,
    base: &[[f64; 3]],
    tolerance: f64,
    profile_valid: bool,
) -> Option<cadkernel::brep::Body> {
    if solid.thickness.abs() <= 1.0e-10 || !profile_valid {
        return None;
    }
    let normal = glam::DVec3::new(solid.normal.x, solid.normal.y, solid.normal.z)
        .normalize_or(glam::DVec3::Z);
    let origin = glam::DVec3::from_array(base[0]);
    let axis = base
        .iter()
        .skip(1)
        .map(|point| glam::DVec3::from_array(*point) - origin)
        .find(|axis| axis.length() > tolerance)?;
    let plane = cadkernel::space::Plane::orthonormal(
        base[0],
        axis.to_array(),
        normal.to_array(),
    )?;
    if base
        .iter()
        .any(|point| !plane.contains(*point, tolerance))
    {
        return None;
    }
    let projected = base
        .iter()
        .map(|point| plane.project(*point))
        .collect::<Option<Vec<_>>>()?;
    let profile = (0..projected.len())
        .map(|index| {
            cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
                start: projected[index],
                end: projected[(index + 1) % projected.len()],
            })
        })
        .collect::<Vec<_>>();
    cadkernel::brep::extrude(
        plane,
        &profile,
        (normal * solid.thickness).to_array(),
    )
}

fn solid_geometry(solid: &Solid) -> Option<SolidGeometry> {
    let base = boundary(solid);
    let tolerance = kernel_tolerance(&base);
    let fill_tris = cadkernel::space::polygon::triangulate(
        &base,
        cadkernel::geom2d::Tolerance::new(tolerance),
    );
    if solid.thickness.abs() <= 1.0e-10 {
        return Some(SolidGeometry {
            edges: boundary_edges(&base),
            fill_tris,
            extruded: false,
        });
    }
    let Some(body) = extrusion_body(solid, &base, tolerance, !fill_tris.is_empty()) else {
        return Some(SolidGeometry {
            edges: boundary_edges(&base),
            fill_tris: Vec::new(),
            extruded: false,
        });
    };
    let mesh = cadkernel::brep::mesh::tessellate(
        &body,
        cadkernel::brep::mesh::TessellationTolerance::new(
            cadkernel::tessellation::DEFAULT_ANGLE,
            tolerance,
        ),
    );
    if !mesh.missing_faces.is_empty() || mesh.mesh.is_empty() {
        return Some(SolidGeometry {
            edges: boundary_edges(&base),
            fill_tris: Vec::new(),
            extruded: false,
        });
    }
    let edges = mesh
        .edges
        .iter()
        .flat_map(|edge| edge.positions.windows(2).map(|pair| (pair[0], pair[1])))
        .collect();
    let mut fill_tris = Vec::with_capacity(mesh.mesh.triangles.len() * 3);
    for triangle in mesh.mesh.triangles {
        fill_tris.extend(triangle.map(|index| mesh.mesh.positions[index]));
    }
    Some(SolidGeometry {
        edges,
        fill_tris,
        extruded: true,
    })
}

pub(crate) fn wcs_bounds(solid: &Solid) -> Option<cadkernel::brep::Aabb> {
    let base = boundary(solid);
    let tolerance = kernel_tolerance(&base);
    let profile_valid = !cadkernel::space::polygon::triangulate(
        &base,
        cadkernel::geom2d::Tolerance::new(tolerance),
    )
    .is_empty();
    extrusion_body(solid, &base, tolerance, profile_valid)
        .and_then(|body| cadkernel::brep::body_bounds(&body))
        .or_else(|| cadkernel::brep::Aabb::around(base))
}

impl RenderConvertible for Solid {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let geometry = solid_geometry(self)?;
        let mut lines = Vec::new();
        for &(start, end) in &geometry.edges {
            push_edge(&mut lines, start, end);
        }
        let mut nodes = Vec::new();
        for &(start, end) in &geometry.edges {
            for point in [start, end] {
                if !nodes.contains(&point) {
                    nodes.push(point);
                }
            }
        }
        let mut snap = Vec::with_capacity(nodes.len() * 2 + geometry.edges.len());
        for point in nodes {
            snap.push((dvec3(point), SnapHint::Node));
            snap.push((dvec3(point), SnapHint::Endpoint));
        }
        snap.extend(geometry.edges.iter().map(|(start, end)| {
            ((dvec3(*start) + dvec3(*end)) * 0.5, SnapHint::Midpoint)
        }));
        let pick_tris = geometry
            .extruded
            .then(|| geometry.fill_tris.clone())
            .unwrap_or_default();

        Some(RenderEntity {
            pick_tris,
            object: RenderObject::Lines(lines),
            snap_pts: snap,
            tangent_geoms: vec![],
            key_vertices: Vec::new(),
            fill_tris: geometry.fill_tris,
        })
    }
}

impl Grippable for Solid {
    fn grips(&self) -> Vec<GripDef> {
        let corners = wcs_corners(self);
        vec![
            square_grip(0, dvec3(corners[0])),
            square_grip(1, dvec3(corners[1])),
            square_grip(2, dvec3(corners[2])),
            square_grip(3, dvec3(corners[3])),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let Some(current) = wcs_corners(self).get(grip_id).copied() else {
            return;
        };
        let point = match apply {
            GripApply::Translate(delta) => dvec3(current) + delta,
            GripApply::Absolute(point) => point,
        };
        set_wcs_corner(self, grip_id, point);
    }
}

impl PropertyEditable for Solid {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let corners = wcs_corners(self);
        let elevation = self.first_corner.z;
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Point 1 X").as_ref(), "sl_p1x", corners[0][0]),
                edit(t!("Point 1 Y").as_ref(), "sl_p1y", corners[0][1]),
                edit(t!("Point 1 Z").as_ref(), "sl_p1z", corners[0][2]),
                edit(t!("Point 2 X").as_ref(), "sl_p2x", corners[1][0]),
                edit(t!("Point 2 Y").as_ref(), "sl_p2y", corners[1][1]),
                edit(t!("Point 2 Z").as_ref(), "sl_p2z", corners[1][2]),
                edit(t!("Point 3 X").as_ref(), "sl_p3x", corners[2][0]),
                edit(t!("Point 3 Y").as_ref(), "sl_p3y", corners[2][1]),
                edit(t!("Point 3 Z").as_ref(), "sl_p3z", corners[2][2]),
                edit(t!("Point 4 X").as_ref(), "sl_p4x", corners[3][0]),
                edit(t!("Point 4 Y").as_ref(), "sl_p4y", corners[3][1]),
                edit(t!("Point 4 Z").as_ref(), "sl_p4z", corners[3][2]),
                edit(t!("Elevation").as_ref(), "sl_elev", elevation),
                edit(t!("Thickness").as_ref(), "sl_thickness", self.thickness),
                edit(t!("Normal X").as_ref(), "sl_normal_x", self.normal.x),
                edit(t!("Normal Y").as_ref(), "sl_normal_y", self.normal.y),
                edit(t!("Normal Z").as_ref(), "sl_normal_z", self.normal.z),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        let point_field = match field {
            "sl_p1x" => Some((0, 0)),
            "sl_p1y" => Some((0, 1)),
            "sl_p1z" => Some((0, 2)),
            "sl_p2x" => Some((1, 0)),
            "sl_p2y" => Some((1, 1)),
            "sl_p2z" => Some((1, 2)),
            "sl_p3x" => Some((2, 0)),
            "sl_p3y" => Some((2, 1)),
            "sl_p3z" => Some((2, 2)),
            "sl_p4x" => Some((3, 0)),
            "sl_p4y" => Some((3, 1)),
            "sl_p4z" => Some((3, 2)),
            _ => None,
        };
        if let Some((point_index, component)) = point_field {
            let mut point = dvec3(wcs_corners(self)[point_index]);
            point[component] = v;
            set_wcs_corner(self, point_index, point);
            return;
        }

        match field {
            "sl_elev" => {
                let delta = v - self.first_corner.z;
                self.first_corner.z += delta;
                self.second_corner.z += delta;
                self.third_corner.z += delta;
                self.fourth_corner.z += delta;
            }
            "sl_thickness" => self.thickness = v,
            "sl_normal_x" | "sl_normal_y" | "sl_normal_z" => {
                let world = wcs_corners(self);
                let mut normal = glam::DVec3::new(self.normal.x, self.normal.y, self.normal.z);
                match field {
                    "sl_normal_x" => normal.x = v,
                    "sl_normal_y" => normal.y = v,
                    "sl_normal_z" => normal.z = v,
                    _ => {}
                }
                if normal.length_squared() > 1.0e-20 {
                    normal = normal.normalize();
                    self.normal.x = normal.x;
                    self.normal.y = normal.y;
                    self.normal.z = normal.z;
                    for (index, point) in world.into_iter().enumerate() {
                        set_wcs_corner(self, index, dvec3(point));
                    }
                }
            }
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
