use acadrust::entities::Point;
use acadrust::EntityType;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, parse_f64, square_grip};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::SnapHint;

/// Nominal viewport height (px) used to turn a relative PDSIZE percentage into
/// an on-screen pixel size. The exact viewport height isn't threaded into
/// tessellation; this reference keeps relative points a roughly constant
/// fraction of the screen across zoom levels.
const REL_REF_PX: f64 = 600.0;

/// Resolve a positive (absolute) PDSIZE to a world size. Relative/zero PDSIZE
/// is handled by [`relative_render`] when a world-per-pixel factor is available;
/// without one it falls back to a small fixed world size.
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
    if pdmode == 0 {
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
    let pts = point_glyph(wx, wy, wz, pdmode, s);
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
        key_vertices: vec![[wx, wy, wz]],
        fill_tris: vec![],
    }
}

/// Viewport-aware override for a relative (≤ 0) PDSIZE: size the glyph from the
/// world-per-pixel factor so the point stays a roughly constant on-screen size
/// across zoom. Returns `None` for an absolute PDSIZE, the size-independent
/// default dot (PDMODE 0), or when no `wpp` is available — the caller then uses
/// the normal header-driven path.
pub fn relative_render(
    entity: &EntityType,
    document: &acadrust::CadDocument,
    wpp: Option<f32>,
) -> Option<RenderEntity> {
    let EntityType::Point(pt) = entity else {
        return None;
    };
    let pdsize = document.header.point_display_size;
    let pdmode = document.header.point_display_mode;
    if pdsize > 0.0 || pdmode == 0 {
        return None;
    }
    let wpp = wpp.filter(|w| *w > 0.0)?;
    Some(point_render(pt, pdmode, relative_world_size(pdsize, wpp) * 0.5))
}

/// Full on-screen glyph size (world units) for a relative (≤ 0) PDSIZE at the
/// given world-per-pixel factor. PDSIZE 0 is the 5% default; negative is the
/// percentage. Used both for rendering and to seed an absolute size when the
/// user switches the Point Style dialog to absolute units.
pub fn relative_world_size(pdsize: f64, wpp: f32) -> f64 {
    let pct = if pdsize == 0.0 { 5.0 } else { -pdsize };
    (pct / 100.0) * REL_REF_PX * wpp as f64
}

fn point_glyph(cx: f64, cy: f64, z: f64, pdmode: i16, s_half: f64) -> Vec<[f64; 3]> {
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
    let nan = [f64::NAN, f64::NAN, f64::NAN];
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut push_seg = |a: [f64; 3], b: [f64; 3]| {
        if !pts.is_empty() {
            pts.push(nan);
        }
        pts.push(a);
        pts.push(b);
    };
    match shape {
        // 0 = single dot — emit a tiny "+" so it's visible at any zoom.
        0 => {
            let d = s * 0.05;
            push_seg([cx - d, cy, z], [cx + d, cy, z]);
            push_seg([cx, cy - d, z], [cx, cy + d, z]);
        }
        1 => {} // explicit nothing
        2 => {
            push_seg([cx - arm, cy, z], [cx + arm, cy, z]);
            push_seg([cx, cy - arm, z], [cx, cy + arm, z]);
        }
        3 => {
            push_seg([cx - arm, cy - arm, z], [cx + arm, cy + arm, z]);
            push_seg([cx - arm, cy + arm, z], [cx + arm, cy - arm, z]);
        }
        4 => {
            // Upward tick rising from the point (length = PDSIZE/2), not a
            // vertical line centred on it.
            push_seg([cx, cy, z], [cx, cy + s, z]);
        }
        _ => {
            push_seg([cx - s, cy, z], [cx + s, cy, z]);
            push_seg([cx, cy - s, z], [cx, cy + s, z]);
        }
    }
    if circle {
        // 16-segment polyline circle.
        const N: usize = 16;
        let mut ring: Vec<[f64; 3]> = Vec::with_capacity(N + 1);
        for i in 0..=N {
            let a = i as f64 * std::f64::consts::TAU / N as f64;
            ring.push([cx + a.cos() * s, cy + a.sin() * s, z]);
        }
        if !pts.is_empty() {
            pts.push(nan);
        }
        pts.extend(ring);
    }
    if square {
        let p1 = [cx - s, cy - s, z];
        let p2 = [cx + s, cy - s, z];
        let p3 = [cx + s, cy + s, z];
        let p4 = [cx - s, cy + s, z];
        if !pts.is_empty() {
            pts.push(nan);
        }
        pts.extend_from_slice(&[p1, p2, p3, p4, p1]);
    }
    pts
}

fn to_render(pt: &Point, document: &acadrust::CadDocument) -> RenderEntity {
    let pdmode = document.header.point_display_mode;
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
