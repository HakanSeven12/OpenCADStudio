use acadrust::entities::{Ole2Frame, OleObjectType};

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::SnapHint;

fn to_render(ole: &Ole2Frame) -> RenderEntity {
    let x0 = ole.upper_left_corner.x;
    let y0 = ole.lower_right_corner.y;
    let x1 = ole.lower_right_corner.x;
    let y1 = ole.upper_left_corner.y;
    let z = ole.upper_left_corner.z;

    if (x1 - x0).abs() < 1e-6 && (y1 - y0).abs() < 1e-6 {
        let s = 0.5_f64;
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(vec![[-s, 0.0, z], [s, 0.0, z]]),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    // Frame border only — the embedded presentation bitmap is drawn inside the
    // rectangle by the image pass (see `ImageModel::from_ole2frame`). The old
    // diagonal-X placeholder would have crossed over that image.
    let pts: Vec<[f64; 3]> = vec![
        [x0, y0, z],
        [x1, y0, z],
        [x1, y0, z],
        [x1, y1, z],
        [x1, y1, z],
        [x0, y1, z],
        [x0, y1, z],
        [x0, y0, z],
    ];
    let center = glam::DVec3::new(cx, cy, z);
    RenderEntity {
        // Interior pick surface: the frame selects on a click anywhere
        // inside, not just on its border.
        pick_tris: crate::entities::common::quad_pick_tris(&[
            [x0, y0, z],
            [x1, y0, z],
            [x1, y1, z],
            [x0, y1, z],
        ]),
        object: RenderObject::Lines(pts),
        snap_pts: vec![(center, SnapHint::Center)],
        tangent_geoms: vec![],
        key_vertices: vec![[x0, y0, z], [x1, y1, z]],
        fill_tris: vec![],
    }
}

fn grips(ole: &Ole2Frame) -> Vec<GripDef> {
    let ul = glam::DVec3::new(
        ole.upper_left_corner.x,
        ole.upper_left_corner.y,
        ole.upper_left_corner.z,
    );
    let lr = glam::DVec3::new(
        ole.lower_right_corner.x,
        ole.lower_right_corner.y,
        ole.lower_right_corner.z,
    );
    let center = (ul + lr) * 0.5;
    vec![
        square_grip(0, ul),
        square_grip(1, lr),
        center_grip(2, center),
    ]
}

fn properties(ole: &Ole2Frame) -> Vec<PropSection> {
    let type_str = match ole.ole_object_type {
        OleObjectType::Link => "Link",
        OleObjectType::Embedded => "Embedded",
        OleObjectType::Static => "Static",
    };
    let width = (ole.lower_right_corner.x - ole.upper_left_corner.x).abs();
    let height = (ole.upper_left_corner.y - ole.lower_right_corner.y).abs();
    vec![
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("Position X", "ole_ulx", ole.upper_left_corner.x),
                edit("Position Y", "ole_uly", ole.upper_left_corner.y),
                edit("Position Z", "ole_ulz", ole.upper_left_corner.z),
                ro("Width", "ole_width", format!("{:.4}", width)),
                ro("Height", "ole_height", format!("{:.4}", height)),
                ro("Scale width", "ole_scale_width", String::new()),
                ro("Scale height", "ole_scale_height", String::new()),
                ro("Lock aspect", "ole_lock_aspect", String::new()),
            ],
        },
        PropSection {
            title: "Misc".into(),
            props: vec![
                ro("Type", "ole_type", type_str),
                ro("Plot quality", "ole_plot_quality", String::new()),
            ],
        },
    ]
}

fn apply_geom_prop(ole: &mut Ole2Frame, field: &str, value: &str) {
    let Ok(v) = value.trim().parse::<f64>() else {
        return;
    };
    match field {
        "ole_ulx" => ole.upper_left_corner.x = v,
        "ole_uly" => ole.upper_left_corner.y = v,
        "ole_ulz" => ole.upper_left_corner.z = v,
        "ole_lrx" => ole.lower_right_corner.x = v,
        "ole_lry" => ole.lower_right_corner.y = v,
        _ => {}
    }
}

fn apply_grip(ole: &mut Ole2Frame, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            ole.upper_left_corner.x = p.x as f64;
            ole.upper_left_corner.y = p.y as f64;
        }
        (1, GripApply::Absolute(p)) => {
            ole.lower_right_corner.x = p.x as f64;
            ole.lower_right_corner.y = p.y as f64;
        }
        (2, GripApply::Translate(d)) => {
            ole.upper_left_corner.x += d.x as f64;
            ole.upper_left_corner.y += d.y as f64;
            ole.lower_right_corner.x += d.x as f64;
            ole.lower_right_corner.y += d.y as f64;
        }
        _ => {}
    }
}

fn apply_transform(ole: &mut Ole2Frame, t: &EntityTransform) {
    match t {
        EntityTransform::Translate(d) => {
            ole.upper_left_corner.x += d.x as f64;
            ole.upper_left_corner.y += d.y as f64;
            ole.upper_left_corner.z += d.z as f64;
            ole.lower_right_corner.x += d.x as f64;
            ole.lower_right_corner.y += d.y as f64;
            ole.lower_right_corner.z += d.z as f64;
        }
        EntityTransform::Scale { center, factor } => {
            let scale = |v: f64, c: f64| c + (v - c) * (*factor as f64);
            ole.upper_left_corner.x = scale(ole.upper_left_corner.x, center.x as f64);
            ole.upper_left_corner.y = scale(ole.upper_left_corner.y, center.y as f64);
            ole.lower_right_corner.x = scale(ole.lower_right_corner.x, center.x as f64);
            ole.lower_right_corner.y = scale(ole.lower_right_corner.y, center.y as f64);
        }
        EntityTransform::Affine(transform) => {
            acadrust::Entity::apply_transform(ole, transform);
        }
        _ => {}
    }
}

impl RenderConvertible for Ole2Frame {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

crate::impl_entity_basics!(Ole2Frame);

impl crate::entities::traits::FallbackTess for Ole2Frame {
    fn fallback_geometry(&self) -> crate::scene::convert::tess_util::FallbackGeometry {
        // OLE objects carry a bounding rectangle in model space.
        // Render a simple X-through-rectangle placeholder.
        let x0 = self.upper_left_corner.x;
        let y0 = self.lower_right_corner.y;
        let x1 = self.lower_right_corner.x;
        let y1 = self.upper_left_corner.y;
        let z = self.upper_left_corner.z;
        if (x1 - x0).abs() < 1e-6 && (y1 - y0).abs() < 1e-6 {
            // Degenerate / unknown size — show a small cross.
            let s = 0.5_f64;
            return (vec![[-s, 0.0, 0.0], [s, 0.0, 0.0]], vec![], vec![], vec![]);
        }
        let pts = vec![
            // Frame border only; the presentation bitmap fills the rectangle.
            [x0, y0, z],
            [x1, y0, z],
            [x1, y0, z],
            [x1, y1, z],
            [x1, y1, z],
            [x0, y1, z],
            [x0, y1, z],
            [x0, y0, z],
        ];
        (pts, vec![], vec![], vec![[x0, y0, z], [x1, y1, z]])
    }
}
