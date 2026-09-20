use acadrust::entities::{Viewport, ViewportRenderMode};

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};

use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};

// ── Standard scale options ────────────────────────────────────────────────

const STANDARD_SCALES: &[(&str, f64)] = &[
    ("1:500", 0.002),
    ("1:200", 0.005),
    ("1:100", 0.01),
    ("1:50", 0.02),
    ("1:20", 0.05),
    ("1:10", 0.1),
    ("1:5", 0.2),
    ("1:2", 0.5),
    ("1:1", 1.0),
    ("2:1", 2.0),
    ("5:1", 5.0),
    ("10:1", 10.0),
];

fn scale_label(scale: f64) -> String {
    for (label, val) in STANDARD_SCALES {
        if (scale - val).abs() < val * 0.01 {
            return label.to_string();
        }
    }
    format!("{:.6}", scale)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

// ── Render mode options ───────────────────────────────────────────────────

const RENDER_MODES: &[(&str, ViewportRenderMode)] = &[
    ("2D Wireframe", ViewportRenderMode::Wireframe2D),
    ("3D Wireframe", ViewportRenderMode::Wireframe3D),
    ("Hidden Line", ViewportRenderMode::HiddenLine),
    ("Flat Shaded", ViewportRenderMode::FlatShaded),
    ("Gouraud Shaded", ViewportRenderMode::GouraudShaded),
    (
        "Flat Shaded + Edges",
        ViewportRenderMode::FlatShadedWithEdges,
    ),
    (
        "Gouraud Shaded + Edges",
        ViewportRenderMode::GouraudShadedWithEdges,
    ),
];

// ── Shade plot mode labels ────────────────────────────────────────────────

const SHADE_PLOT_LABELS: &[&str] = &["As Displayed", "Wireframe", "Hidden", "Rendered"];

fn shade_plot_label(mode: i16) -> &'static str {
    SHADE_PLOT_LABELS
        .get(mode as usize)
        .copied()
        .unwrap_or("As Displayed")
}

fn grips(vp: &Viewport) -> Vec<GripDef> {
    let (cx, cy, cz) = (vp.center.x, vp.center.y, vp.center.z);
    let hw = vp.width / 2.0;
    let hh = vp.height / 2.0;
    vec![
        center_grip(0, glam::DVec3::new(cx, cy, cz)),
        square_grip(1, glam::DVec3::new(cx + hw, cy + hh, cz)),
        square_grip(2, glam::DVec3::new(cx - hw, cy + hh, cz)),
        square_grip(3, glam::DVec3::new(cx - hw, cy - hh, cz)),
        square_grip(4, glam::DVec3::new(cx + hw, cy - hh, cz)),
    ]
}

fn properties(vp: &Viewport) -> Vec<PropSection> {
    let scale_opts: Vec<String> = STANDARD_SCALES.iter().map(|(s, _)| s.to_string()).collect();
    let effective_scale =
        crate::scene::vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
    let current_scale_label = scale_label(effective_scale);

    let shade_opts: Vec<String> = SHADE_PLOT_LABELS.iter().map(|s| s.to_string()).collect();
    let current_shade = shade_plot_label(vp.shade_plot_mode).to_string();

    let annotation_scale = if effective_scale.abs() > 1e-9 {
        format!("1:{}", scale_label(effective_scale))
    } else {
        String::new()
    };

    vec![
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("Center X", "center_x", vp.center.x),
                edit("Center Y", "center_y", vp.center.y),
                edit("Center Z", "center_z", vp.center.z),
                edit("Height", "vp_h", vp.height),
                edit("Width", "vp_w", vp.width),
            ],
        },
        PropSection {
            title: "Misc".into(),
            props: vec![
                Property {
                    label: "On".into(),
                    field: "vp_on",
                    value: PropValue::BoolToggle {
                        field: "vp_on",
                        value: vp.status.is_on,
                    },
                },
                ro(
                    "Clipped",
                    "vp_clipped",
                    if vp.clip_boundary_handle.is_null() {
                        "No"
                    } else {
                        "Yes"
                    },
                ),
                Property {
                    label: "Display locked".into(),
                    field: "vp_locked",
                    value: PropValue::BoolToggle {
                        field: "vp_locked",
                        value: vp.status.locked,
                    },
                },
                ro("Annotation scale", "vp_anno_scale", annotation_scale),
                Property {
                    label: "Standard scale".into(),
                    field: "vscale_std",
                    value: PropValue::Choice {
                        selected: current_scale_label,
                        options: scale_opts,
                    },
                },
                edit("Custom scale", "vscale", effective_scale),
                Property {
                    label: "UCS per viewport".into(),
                    field: "vp_ucs_per_vp",
                    value: PropValue::BoolToggle {
                        field: "vp_ucs_per_vp",
                        value: vp.ucs_per_viewport,
                    },
                },
                ro(
                    "Layer property overrides",
                    "vp_layer_overrides",
                    if vp.frozen_layers.is_empty() {
                        "No".to_string()
                    } else {
                        format!("Yes ({})", vp.frozen_layers.len())
                    },
                ),
                ro(
                    "Visual style",
                    "vp_visual_style_handle",
                    if vp.visual_style_handle.is_null() {
                        "(none)".to_string()
                    } else {
                        format!("{:X}", vp.visual_style_handle.value())
                    },
                ),
                Property {
                    label: "Shade plot".into(),
                    field: "vp_shade_plot",
                    value: PropValue::Choice {
                        selected: current_shade,
                        options: shade_opts,
                    },
                },
                ro("Linked to Sheet View", "vp_sheet_view", String::new()),
            ],
        },
    ]
}

fn apply_geom_prop(vp: &mut Viewport, field: &str, value: &str) {
    use acadrust::types::Vector3;

    // Boolean / toggle fields handled first (value = "toggle" or "true"/"false").
    match field {
        "vp_locked" => {
            vp.status.locked = if value == "toggle" {
                !vp.status.locked
            } else {
                value == "true"
            };
            return;
        }
        "vp_on" => {
            vp.status.is_on = if value == "toggle" {
                !vp.status.is_on
            } else {
                value == "true"
            };
            return;
        }
        "vp_perspective" => {
            vp.status.perspective = if value == "toggle" {
                !vp.status.perspective
            } else {
                value == "true"
            };
            return;
        }
        "vp_hide_plot" => {
            vp.status.hide_plot = if value == "toggle" {
                !vp.status.hide_plot
            } else {
                value == "true"
            };
            return;
        }
        "vp_ucs_icon" => {
            vp.ucs_icon_visible = if value == "toggle" {
                !vp.ucs_icon_visible
            } else {
                value == "true"
            };
            return;
        }
        "vp_ucs_per_vp" => {
            vp.ucs_per_viewport = if value == "toggle" {
                !vp.ucs_per_viewport
            } else {
                value == "true"
            };
            return;
        }
        _ => {}
    }

    // Render mode picker.
    if field == "vp_render" {
        if let Some(&(_, mode)) = RENDER_MODES.iter().find(|(label, _)| *label == value) {
            vp.render_mode = mode;
        }
        return;
    }

    // Shade plot mode picker.
    if field == "vp_shade_plot" {
        if let Some(idx) = SHADE_PLOT_LABELS.iter().position(|&s| s == value) {
            vp.shade_plot_mode = idx as i16;
        }
        return;
    }

    // Standard view direction picker.
    if field == "vp_view" {
        let dir: Option<(f64, f64, f64)> = match value {
            "Top" => Some((0.0, 0.0, 1.0)),
            "Bottom" => Some((0.0, 0.0, -1.0)),
            "Front" => Some((0.0, -1.0, 0.0)),
            "Back" => Some((0.0, 1.0, 0.0)),
            "Left" => Some((-1.0, 0.0, 0.0)),
            "Right" => Some((1.0, 0.0, 0.0)),
            "SW Isometric" => Some((-1.0, -1.0, 1.0)),
            "SE Isometric" => Some((1.0, -1.0, 1.0)),
            "NE Isometric" => Some((1.0, 1.0, 1.0)),
            "NW Isometric" => Some((-1.0, 1.0, 1.0)),
            _ => None,
        };
        if let Some((dx, dy, dz)) = dir {
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            vp.view_direction = Vector3::new(dx / len, dy / len, dz / len);
        }
        return;
    }

    // Numeric fields.
    let Some(v) = parse_f64(value) else { return };
    match field {
        "center_x" => vp.center.x = v,
        "center_y" => vp.center.y = v,
        "center_z" => vp.center.z = v,
        "vp_w" if v > 0.0 => vp.width = v,
        "vp_h" if v > 0.0 => vp.height = v,
        "vscale" if v > 0.0 => {
            vp.custom_scale = v;
            vp.view_height = vp.height / v;
        }
        "vtgt_x" => vp.view_target.x = v,
        "vtgt_z" => vp.view_target.z = v,
        "vp_lens" if v > 0.0 => vp.lens_length = v,
        "vp_ucs_ox" => vp.ucs_origin.x = v,
        "vp_ucs_oy" => vp.ucs_origin.y = v,
        "vp_ucs_oz" => vp.ucs_origin.z = v,
        "vp_ucs_xx" => vp.ucs_x_axis.x = v,
        "vp_ucs_xy" => vp.ucs_x_axis.y = v,
        "vp_ucs_xz" => vp.ucs_x_axis.z = v,
        "vp_ucs_yx" => vp.ucs_y_axis.x = v,
        "vp_ucs_yy" => vp.ucs_y_axis.y = v,
        "vp_ucs_yz" => vp.ucs_y_axis.z = v,
        "vp_snap_bx" => vp.snap_base.x = v,
        "vp_snap_by" => vp.snap_base.y = v,
        "vp_snap_sx" if v >= 0.0 => vp.snap_spacing.x = v,
        "vp_snap_sy" if v >= 0.0 => vp.snap_spacing.y = v,
        "vp_snap_ang" => vp.snap_angle = v.to_radians(),
        "vp_twist" => vp.twist_angle = v.to_radians(),
        "vp_front_clip" => vp.front_clip_z = v,
        "vp_back_clip" => vp.back_clip_z = v,
        "vp_circle_sides" if v >= 0.0 && v <= i16::MAX as f64 => {
            vp.circle_sides = v as i16;
        }
        _ => {}
    }
}

fn apply_grip(vp: &mut Viewport, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Translate(d)) => {
            vp.center.x += d.x as f64;
            vp.center.y += d.y as f64;
            vp.center.z += d.z as f64;
        }
        (0, GripApply::Absolute(p)) => {
            vp.center.x = p.x as f64;
            vp.center.y = p.y as f64;
            vp.center.z = p.z as f64;
        }
        (1..=4, GripApply::Absolute(p)) => {
            let hw = vp.width * 0.5;
            let hh = vp.height * 0.5;
            let opposite = match grip_id {
                1 => (vp.center.x - hw, vp.center.y - hh),
                2 => (vp.center.x + hw, vp.center.y - hh),
                3 => (vp.center.x + hw, vp.center.y + hh),
                4 => (vp.center.x - hw, vp.center.y + hh),
                _ => unreachable!(),
            };
            resize_from_corners(vp, opposite, (p.x as f64, p.y as f64));
        }
        _ => {}
    }
}

/// Stretch a rectangular paper-space viewport by the corners captured by a
/// crossing window. Bounds that have no captured corner stay anchored.
pub fn stretch(
    vp: &mut Viewport,
    win_min: glam::DVec3,
    win_max: glam::DVec3,
    delta: glam::DVec3,
) -> bool {
    let hw = vp.width * 0.5;
    let hh = vp.height * 0.5;
    let (left, right) = (vp.center.x - hw, vp.center.x + hw);
    let (bottom, top) = (vp.center.y - hh, vp.center.y + hh);
    let inside =
        |x: f64, y: f64| x >= win_min.x && x <= win_max.x && y >= win_min.y && y <= win_max.y;

    let bottom_left = inside(left, bottom);
    let bottom_right = inside(right, bottom);
    let top_left = inside(left, top);
    let top_right = inside(right, top);
    if !(bottom_left || bottom_right || top_left || top_right) {
        return false;
    }

    let moved_left = if bottom_left || top_left {
        left + delta.x
    } else {
        left
    };
    let moved_right = if bottom_right || top_right {
        right + delta.x
    } else {
        right
    };
    let moved_bottom = if bottom_left || bottom_right {
        bottom + delta.y
    } else {
        bottom
    };
    let moved_top = if top_left || top_right {
        top + delta.y
    } else {
        top
    };
    resize_from_corners(vp, (moved_left, moved_bottom), (moved_right, moved_top))
}

/// Rebuild center and size from two opposite corners while preserving viewport
/// scale as its paper-space height changes.
fn resize_from_corners(vp: &mut Viewport, a: (f64, f64), b: (f64, f64)) -> bool {
    let new_width = (b.0 - a.0).abs();
    let new_height = (b.1 - a.1).abs();
    let mut changed = false;

    if new_width > 0.01 {
        let new_center_x = (a.0 + b.0) * 0.5;
        changed |=
            (vp.center.x - new_center_x).abs() > 1e-12 || (vp.width - new_width).abs() > 1e-12;
        vp.center.x = new_center_x;
        vp.width = new_width;
    }
    if new_height > 0.01 {
        let old_height = vp.height;
        let new_center_y = (a.1 + b.1) * 0.5;
        changed |=
            (vp.center.y - new_center_y).abs() > 1e-12 || (old_height - new_height).abs() > 1e-12;
        vp.center.y = new_center_y;
        if old_height > 1e-9 && vp.view_height.abs() > 1e-9 {
            vp.view_height *= new_height / old_height;
        }
        vp.height = new_height;
    }

    changed
}

fn apply_transform(vp: &mut Viewport, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(vp, t, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.center.x,
            &mut entity.center.y,
            p1,
            p2,
        );
    });
}

crate::impl_entity_basics!(Viewport);

impl crate::entities::traits::FallbackTess for Viewport {
    fn fallback_geometry(&self) -> crate::scene::convert::tess_util::FallbackGeometry {
        // A clipped viewport uses its linked boundary entity as its visible
        // frame. Drawing the viewport's rectangular extents as well leaves an
        // incorrect box around polygonal/Object MVIEW results.
        if !self.clip_boundary_handle.is_null() {
            return (vec![], vec![], vec![], vec![]);
        }
        let cx = self.center.x;
        let cy = self.center.y;
        let cz = self.center.z;
        let hw = self.width / 2.0;
        let hh = self.height / 2.0;
        let pts = vec![
            [cx - hw, cy - hh, cz],
            [cx + hw, cy - hh, cz],
            [cx + hw, cy + hh, cz],
            [cx - hw, cy + hh, cz],
            [cx - hw, cy - hh, cz],
        ];
        (pts, vec![], vec![], vec![])
    }
}
