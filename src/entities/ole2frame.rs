use codec::entities::{Ole2Frame, OleObjectType};

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::t;
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

/// What an OLE object keeps while its drawing is open: the size its scales
/// are measured against (unknown for an object read from a file until it is
/// first resized, so its scales show 0) and whether resizing keeps its
/// aspect ratio.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameState {
    pub base: Option<(f64, f64)>,
    pub lock_aspect: bool,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            base: None,
            lock_aspect: true,
        }
    }
}

/// OLEHIDE: 0 every OLE object shows, 1 only those in paper space, 2 only
/// those in model space, 3 none (a profile setting).
static OLE_HIDE: std::sync::atomic::AtomicI16 = std::sync::atomic::AtomicI16::new(0);

pub(crate) fn hide_mode() -> i16 {
    OLE_HIDE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_hide_mode(value: i16) {
    OLE_HIDE.store(value.clamp(0, 3), std::sync::atomic::Ordering::Relaxed);
}

/// Whether OLEHIDE hides this object.
pub(crate) fn hidden(ole: &Ole2Frame) -> bool {
    match hide_mode() {
        1 => !ole.is_paper_space,
        2 => ole.is_paper_space,
        3 => true,
        _ => false,
    }
}

/// OLEFRAME of the drawing: 0 no frame, 1 frame shown and plotted, 2 frame
/// shown but not plotted (the default).
pub(crate) fn frame_mode(document: &codec::Document) -> i16 {
    crate::io::drawing_variable(document, "OLEFRAME")
        .and_then(|value| value.trim().parse::<i16>().ok())
        .filter(|value| (0..=2).contains(value))
        .unwrap_or(2)
}

/// Lower-left corner, width and height of the frame.
pub(crate) fn frame_box(ole: &Ole2Frame) -> (glam::DVec3, f64, f64) {
    let x = ole.upper_left_corner.x.min(ole.lower_right_corner.x);
    let y = ole.upper_left_corner.y.min(ole.lower_right_corner.y);
    (
        glam::DVec3::new(x, y, ole.upper_left_corner.z),
        (ole.lower_right_corner.x - ole.upper_left_corner.x).abs(),
        (ole.upper_left_corner.y - ole.lower_right_corner.y).abs(),
    )
}

/// Resizes the frame about its upper-left corner, which stays.
pub(crate) fn set_size(ole: &mut Ole2Frame, width: f64, height: f64) {
    ole.lower_right_corner.x = ole.upper_left_corner.x + width;
    ole.lower_right_corner.y = ole.upper_left_corner.y - height;
}

/// The Plot quality choices, by the value the object stores (DXF 73, the
/// byte after the OLE data in DWG).
pub(crate) fn plot_quality_labels() -> [String; 3] {
    [
        crate::tr!("ole-props", "plot-monochrome"),
        crate::tr!("ole-props", "plot-low"),
        crate::tr!("ole-props", "plot-high"),
    ]
}

/// What the Properties palette refuses for an OLE object.
pub(crate) fn validate_property(field: &str, value: &str) -> Result<(), &'static str> {
    match field {
        "ole_width" | "ole_height" | "ole_scale_width" | "ole_scale_height" => {
            match crate::entities::common::parse_f64(value) {
                Some(v) if v > 0.0 => Ok(()),
                _ => Err("Invalid input"),
            }
        }
        _ => Ok(()),
    }
}

fn properties(ole: &Ole2Frame) -> Vec<PropSection> {
    use crate::scene::model::object::{PropValue, Property};
    let type_str = match ole.ole_object_type {
        OleObjectType::Link => crate::tr!("ole-props", "type-link"),
        OleObjectType::Embedded => crate::tr!("ole-props", "type-embedded"),
        OleObjectType::Static => crate::tr!("ole-props", "type-static"),
    };
    let (corner, width, height) = frame_box(ole);
    let labels = plot_quality_labels();
    let quality = labels[usize::from(ole.lock_aspect.min(2))].clone();
    let yes_no = |on: bool| PropValue::Choice {
        selected: if on { t!("Yes") } else { t!("No") }.into_owned(),
        options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
    };
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Position X").as_ref(), "ole_x", corner.x),
                edit(t!("Position Y").as_ref(), "ole_y", corner.y),
                edit(t!("Position Z").as_ref(), "ole_z", corner.z),
                edit(t!("Width").as_ref(), "ole_width", width),
                edit(t!("Height").as_ref(), "ole_height", height),
                // The scales and the aspect lock live with the open drawing;
                // the Properties panel fills them in.
                crate::entities::common::edit_scalar_prop(
                    &crate::tr!("ole-props", "scale-width"),
                    "ole_scale_width",
                    0.0,
                ),
                crate::entities::common::edit_scalar_prop(
                    &crate::tr!("ole-props", "scale-height"),
                    "ole_scale_height",
                    0.0,
                ),
                Property {
                    label: crate::tr!("ole-props", "lock-aspect"),
                    field: "ole_lock_aspect",
                    value: yes_no(true),
                },
            ],
        },
        PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                ro(t!("Type").as_ref(), "ole_type", type_str),
                Property {
                    label: crate::tr!("ole-props", "plot-quality"),
                    field: "ole_plot_quality",
                    value: PropValue::Choice {
                        selected: quality,
                        options: labels.to_vec(),
                    },
                },
                ro(
                    &crate::tr!("ole-props", "source-app"),
                    "ole_source_app",
                    ole.source_application.clone(),
                ),
            ],
        },
    ]
}

fn apply_geom_prop(ole: &mut Ole2Frame, field: &str, value: &str) {
    if field == "ole_plot_quality" {
        if let Some(index) = plot_quality_labels().iter().position(|label| label == value) {
            ole.lock_aspect = index as u8;
        } else if let Ok(index) = value.trim().parse::<u8>() {
            ole.lock_aspect = index.min(2);
        }
        return;
    }
    let Some(v) = crate::entities::common::parse_f64(value) else {
        return;
    };
    let (corner, width, height) = frame_box(ole);
    let shift = match field {
        "ole_x" => glam::DVec3::new(v - corner.x, 0.0, 0.0),
        "ole_y" => glam::DVec3::new(0.0, v - corner.y, 0.0),
        "ole_z" => glam::DVec3::new(0.0, 0.0, v - corner.z),
        "ole_width" if v > 0.0 => return set_size(ole, v, height),
        "ole_height" if v > 0.0 => return set_size(ole, width, v),
        _ => return,
    };
    for point in [&mut ole.upper_left_corner, &mut ole.lower_right_corner] {
        point.x += shift.x;
        point.y += shift.y;
        point.z += shift.z;
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
            codec::Entity::apply_transform(ole, transform);
        }
        _ => {}
    }
}

impl RenderConvertible for Ole2Frame {
    fn to_render(&self, document: &codec::Document) -> Option<RenderEntity> {
        if hidden(self) {
            return None;
        }
        let mut render = to_render(self);
        // OLEFRAME 0: no frame; the picture inside still selects the object.
        if frame_mode(document) == 0 {
            render.object = RenderObject::Lines(Vec::new());
        }
        Some(render)
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
