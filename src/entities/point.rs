use acadrust::entities::Point;
use acadrust::EntityType;
use cadkernel::geom2d::{Circle, Curve, Line};
use cadkernel::space::{curve::bezier_points, PlanarCurve, Plane, Vec3};

use crate::t;
use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, parse_f64, square_grip};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::{PointMarker, SnapHint};

/// Resolve a positive (absolute) PDSIZE to a world size. Relative/zero PDSIZE
/// is handled by [`relative_render`].
fn pdsize_world(pdsize: f64) -> f64 {
    if pdsize > 0.0 {
        pdsize
    } else {
        2.0
    }
}

/// Build the render entity for a point given the glyph half-size `s` in world
/// units. Shared by the header-driven path ([`to_render`]) and the
/// viewport-aware relative path ([`relative_render`]).
fn point_render(pt: &Point, pdmode: i16, s: f64) -> RenderEntity {
    // POINT location is stored in WCS (the extrusion normal only orients the
    // glyph/thickness) — remapping it through the arbitrary-axis OCS moved
    // mirrored points (normal 0,0,-1) to the wrong side of the drawing.
    let (wx, wy, wz) = (pt.location.x, pt.location.y, pt.location.z);
    let snap = glam::DVec3::new(wx, wy, wz);
    let normal = point_normal(pt);
    let top = snap + normal * pt.thickness;
    if pdmode == 0 && pt.thickness.abs() <= 1.0e-10 {
        // Default: a single position (the driver sizes the dot in pixels).
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Dot([wx, wy, wz]),
            snap_pts: vec![(snap, SnapHint::Node)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }
    let pts = point_glyph(pt, pdmode, s);
    if pts.is_empty() {
        // PDMODE 1 = nothing — emit an empty Lines wire so picking still works.
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(vec![]),
            snap_pts: vec![(snap, SnapHint::Node)],
            tangent_geoms: vec![],
            key_vertices: vec![[wx, wy, wz]],
            fill_tris: vec![],
        };
    }
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(pts),
        snap_pts: vec![(snap, SnapHint::Node)],
        tangent_geoms: vec![],
        key_vertices: if pt.thickness.abs() > 1.0e-10 {
            vec![[wx, wy, wz], [top.x, top.y, top.z]]
        } else {
            vec![[wx, wy, wz]]
        },
        fill_tris: vec![],
    }
}

/// Build a unit-sized glyph for a relative (≤ 0) PDSIZE. The wire shader scales
/// its planar displacement from the point origin using the live viewport size,
/// so zooming and resizing do not require retessellation.
pub fn relative_render(
    entity: &EntityType,
    document: &acadrust::CadDocument,
    _wpp: Option<f32>,
) -> Option<RenderEntity> {
    let EntityType::Point(pt) = entity else {
        return None;
    };
    let pdsize = document.header.point_display_size;
    let pdmode = effective_pdmode(pt, document.header.point_display_mode);
    if pdsize > 0.0 || pdmode == 0 {
        return None;
    }
    Some(point_render(pt, pdmode, 0.5))
}

/// Full glyph size in world units for a relative PDSIZE at the current
/// viewport height. Used when the style dialog converts a relative value to an
/// absolute one.
pub fn relative_world_size(pdsize: f64, wpp: f32, viewport_height_px: f32) -> f64 {
    let pct = if pdsize == 0.0 { 5.0 } else { -pdsize };
    (pct / 100.0) * viewport_height_px.max(1.0) as f64 * wpp as f64
}

/// Percentage and plane normal encoded into the point wire for live GPU
/// viewport scaling. A zero PDSIZE means the standard five-percent size.
pub fn relative_marker_spec(
    entity: &EntityType,
    document: &acadrust::CadDocument,
) -> Option<PointMarker> {
    let EntityType::Point(pt) = entity else {
        return None;
    };
    let size = document.header.point_display_size;
    if size > 0.0 || effective_pdmode(pt, document.header.point_display_mode) == 0 {
        return None;
    }
    let plane = point_plane(pt);
    let normal = Vec3::from(plane.normal().unwrap_or([0.0, 0.0, 1.0]));
    Some(PointMarker {
        origin: glam::DVec3::new(pt.location.x, pt.location.y, pt.location.z),
        normal: glam::DVec3::new(normal.x, normal.y, normal.z),
        axis_x: glam::DVec3::from_array(plane.x_axis),
        axis_y: glam::DVec3::from_array(plane.y_axis),
        viewport_percent: if size == 0.0 { 5.0 } else { -size as f32 },
    })
}

fn point_normal(pt: &Point) -> glam::DVec3 {
    let normal = Vec3::new(pt.normal.x, pt.normal.y, pt.normal.z)
        .normalize()
        .unwrap_or(Vec3::Z);
    glam::DVec3::new(normal.x, normal.y, normal.z)
}

fn point_plane(pt: &Point) -> Plane {
    let origin = [pt.location.x, pt.location.y, pt.location.z];
    let normal = Vec3::new(pt.normal.x, pt.normal.y, pt.normal.z)
        .normalize()
        .unwrap_or(Vec3::Z);
    let x_seed = if normal.x.abs() < 1.0 / 64.0 && normal.y.abs() < 1.0 / 64.0 {
        Vec3::Y.cross(normal)
    } else {
        Vec3::Z.cross(normal)
    };
    let base = Plane::orthonormal(origin, x_seed.to_array(), normal.to_array())
        .unwrap_or(Plane::XY);
    let (sin, cos) = pt.x_axis_angle.sin_cos();
    Plane::from_axes(
        origin,
        base.vector_at([cos, sin]),
        base.vector_at([-sin, cos]),
    )
}

fn point_glyph(pt: &Point, pdmode: i16, s_half: f64) -> Vec<[f64; 3]> {
    // PDMODE bits:
    //   shape:  0=dot, 1=nothing, 2='+', 3='×', 4='|'
    //   +32   = enclose in a circle
    //   +64   = enclose in a square
    //   (+96 = both)
    let shape = (pdmode & 0x0F) as i32;
    let circle = (pdmode & 32) != 0;
    let square = (pdmode & 64) != 0;
    let s = s_half;
    // The '+' and '×' arms reach the full PDSIZE (twice the radius), so the
    // cross pokes out past any enclosing circle/square, which sit at the radius.
    let arm = 2.0 * s_half;
    let mut curves = Vec::new();
    let line = |start, end| Curve::Line(Line { start, end });
    match shape {
        0 => {
            let d = s * 0.05;
            curves.push(line([-d, 0.0], [d, 0.0]));
            curves.push(line([0.0, -d], [0.0, d]));
        }
        1 => {}
        2 => {
            curves.push(line([-arm, 0.0], [arm, 0.0]));
            curves.push(line([0.0, -arm], [0.0, arm]));
        }
        3 => {
            curves.push(line([-arm, -arm], [arm, arm]));
            curves.push(line([-arm, arm], [arm, -arm]));
        }
        4 => {
            curves.push(line([0.0, 0.0], [0.0, s]));
        }
        _ => {
            curves.push(line([-s, 0.0], [s, 0.0]));
            curves.push(line([0.0, -s], [0.0, s]));
        }
    }
    if circle {
        curves.push(Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: s,
        }));
    }
    if square {
        curves.push(line([-s, -s], [s, -s]));
        curves.push(line([s, -s], [s, s]));
        curves.push(line([s, s], [-s, s]));
        curves.push(line([-s, s], [-s, -s]));
    }

    let plane = point_plane(pt);
    let nan = [f64::NAN; 3];
    let mut paths: Vec<Vec<[f64; 3]>> = curves
        .iter()
        .map(|curve| PlanarCurve::new(plane, curve.clone()).tessellate(64.0 / std::f64::consts::TAU))
        .collect();
    if pt.thickness.abs() > 1.0e-10 && !curves.is_empty() {
        let normal = Vec3::from(plane.normal().unwrap_or([0.0, 0.0, 1.0]));
        let top_origin = (Vec3::from(plane.origin) + normal * pt.thickness).to_array();
        let top_plane = Plane::from_axes(top_origin, plane.x_axis, plane.y_axis);
        paths.extend(curves.iter().map(|curve| {
            PlanarCurve::new(top_plane, curve.clone())
                .tessellate(64.0 / std::f64::consts::TAU)
        }));
        paths.push(bezier_points(&[plane.origin, top_origin], 1));
    }
    let mut points = Vec::new();
    for path in paths {
        if !points.is_empty() {
            points.push(nan);
        }
        points.extend(path);
    }
    points
}

fn is_defpoints_layer(layer: &str) -> bool {
    let name = layer.rsplit_once('|').map_or(layer, |(_, name)| name);
    name.eq_ignore_ascii_case("DEFPOINTS")
}

/// Definition points always use the dot style.
fn effective_pdmode(pt: &Point, pdmode: i16) -> i16 {
    if is_defpoints_layer(&pt.common.layer) {
        0
    } else {
        pdmode
    }
}

fn to_render(pt: &Point, document: &acadrust::CadDocument) -> RenderEntity {
    let pdmode = effective_pdmode(pt, document.header.point_display_mode);
    let s = pdsize_world(document.header.point_display_size) * 0.5;
    point_render(pt, pdmode, s)
}

fn grips(pt: &Point) -> Vec<GripDef> {
    let p = glam::DVec3::new(pt.location.x, pt.location.y, pt.location.z);
    vec![square_grip(0, p)]
}

fn properties(pt: &Point) -> Vec<PropSection> {
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Position X").as_ref(), "loc_x", pt.location.x),
            edit(t!("Position Y").as_ref(), "loc_y", pt.location.y),
            edit(t!("Position Z").as_ref(), "loc_z", pt.location.z),
        ],
    }]
}

fn apply_geom_prop(pt: &mut Point, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "loc_x" => pt.location.x = v,
        "loc_y" => pt.location.y = v,
        "loc_z" => pt.location.z = v,
        _ => {}
    }
}

fn apply_grip(pt: &mut Point, _grip_id: usize, apply: GripApply) {
    match apply {
        GripApply::Absolute(p) => {
            pt.location.x = p.x as f64;
            pt.location.y = p.y as f64;
            pt.location.z = p.z as f64;
        }
        GripApply::Translate(d) => {
            pt.location.x += d.x as f64;
            pt.location.y += d.y as f64;
            pt.location.z += d.z as f64;
        }
    }
}

fn apply_transform(pt: &mut Point, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(pt, t, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.location.x,
            &mut entity.location.y,
            p1,
            p2,
        );
    });
}

impl RenderConvertible for Point {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self, document))
    }
}

crate::impl_entity_basics!(Point);

#[cfg(test)]
mod tests {
    use super::*;

    fn point_on(layer: &str) -> Point {
        let mut pt = Point::default();
        pt.common.layer = layer.to_string();
        pt.location = acadrust::types::Vector3::new(1.0, 2.0, 0.0);
        pt
    }

    #[test]
    fn definition_points_ignore_the_point_style() {
        let mut doc = acadrust::CadDocument::new();
        doc.header.point_display_mode = 34;
        doc.header.point_display_size = 2.0;

        let drawn = to_render(&point_on("0"), &doc);
        assert!(
            matches!(drawn.object, RenderObject::Lines(ref pts) if !pts.is_empty()),
            "a point the user placed still follows PDMODE"
        );

        for layer in ["Defpoints", "DEFPOINTS", "defpoints", "xref|Defpoints"] {
            let defpoint = to_render(&point_on(layer), &doc);
            assert!(
                matches!(defpoint.object, RenderObject::Dot(_)),
                "a definition point on {layer} must stay a dot"
            );
        }
    }

    #[test]
    fn definition_points_ignore_the_point_style_at_relative_pdsize() {
        let mut doc = acadrust::CadDocument::new();
        doc.header.point_display_mode = 34;
        doc.header.point_display_size = -5.0;

        let drawn = relative_render(&EntityType::Point(point_on("0")), &doc, Some(0.01));
        assert!(drawn.is_some(), "a user point still takes the viewport-aware path");

        let defpoint = relative_render(
            &EntityType::Point(point_on("xref|Defpoints")),
            &doc,
            Some(0.01),
        );
        assert!(
            defpoint.is_none(),
            "a definition point falls through to the dot instead of a sized glyph"
        );
    }
}
