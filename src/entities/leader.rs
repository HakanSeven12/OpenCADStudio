use document::entities::{HooklineDirection, Leader, LeaderCreationType, LeaderPathType};
use document::Entity;

use crate::entities::common::{
    center_grip, edit_prop as edit, ro_prop as ro, square_grip, stepper_prop as stepper,
};
use crate::entities::traits::RenderConvertible;
use crate::entities::transform::EntityTransform;
use crate::display::render_entity::{RenderEntity, RenderObject};
use crate::entities::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::display::wire_model::TangentGeom;
use crate::t;

// ── RenderConvertible (used for snap/grip key-vertices) ─────────────────────

/// Whether a leader draws a horizontal hookline. DWG stores no "hookline
/// exists" flag (only the direction bit), so the stored flag alone would drop
/// every hookline on DWG-loaded leaders: a text-annotated leader gets a
/// horizontal hook whenever its final segment leaves the horizontal by more
/// than 15°.
pub(crate) fn draws_hookline(leader: &Leader) -> bool {
    if leader.hookline_enabled {
        return true;
    }
    let verts = &leader.vertices;
    if leader.creation_type != LeaderCreationType::WithText
        || leader.annotation_handle.is_null()
        || verts.len() < 2
    {
        return false;
    }
    let last = &verts[verts.len() - 1];
    let prev = &verts[verts.len() - 2];
    let (dx, dy) = (last.x - prev.x, last.y - prev.y);
    let len = (dx * dx + dy * dy).sqrt();
    let h = leader.horizontal_direction;
    let hl = (h.x * h.x + h.y * h.y).sqrt();
    let cos = if len > 1e-9 && hl > 1e-9 {
        ((dx * h.x + dy * h.y) / (len * hl)).abs()
    } else {
        1.0
    };
    cos < (15.0f64).to_radians().cos()
}

fn to_render(leader: &Leader) -> RenderEntity {
    let verts = &leader.vertices;
    let nan = [f64::NAN; 3];
    let p3 = |v: &document::types::Vector3| -> [f64; 3] { [v.x, v.y, v.z] };
    let p3f = |v: &document::types::Vector3| -> [f32; 3] { [v.x as f32, v.y as f32, v.z as f32] };

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();

    // Main leader path
    for v in verts {
        points.push(p3(v));
        key_verts.push(p3(v));
    }
    for i in 0..verts.len().saturating_sub(1) {
        // TangentGeom uses f32 (UI-only); cast at construction.
        tangents.push(TangentGeom::Line {
            p1: p3f(&verts[i]),
            p2: p3f(&verts[i + 1]),
        });
    }

    // Arrowhead at vertex[0]
    if leader.arrow_enabled && verts.len() >= 2 {
        let tip = &verts[0];
        let next = &verts[1];
        let dx = next.x - tip.x;
        let dy = next.y - tip.y;
        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (dx, dy) = (dx / len, dy / len);
        // Arrowhead sized to the text height, matching the MLEADER arrowhead.
        let sz = (leader.text_height).max(1.0);
        let a = std::f64::consts::PI / 6.0;
        let (s, c) = a.sin_cos();
        let tip_f = p3(tip);
        points.push(nan);
        points.push([
            tip_f[0] + (dx * c - dy * s) * sz,
            tip_f[1] + (dx * s + dy * c) * sz,
            tip_f[2],
        ]);
        points.push(tip_f);
        points.push([
            tip_f[0] + (dx * c + dy * s) * sz,
            tip_f[1] + (-dx * s + dy * c) * sz,
            tip_f[2],
        ]);
    }

    // Landing line at last vertex
    if draws_hookline(leader) && verts.len() >= 2 {
        let last = verts.last().unwrap();
        let prev = &verts[verts.len() - 2];
        // Landing runs along the leader's horizontal direction (UCS X for
        // UCS-placed leaders, world X otherwise), on the side the leader
        // approaches from.
        let (hx, hy) = {
            let h = leader.horizontal_direction;
            let l = (h.x * h.x + h.y * h.y).sqrt();
            if l > 1e-9 {
                (h.x / l, h.y / l)
            } else {
                (1.0, 0.0)
            }
        };
        let sign = if (last.x - prev.x) * hx + (last.y - prev.y) * hy >= 0.0 {
            1.0_f64
        } else {
            -1.0_f64
        };
        let len = leader.text_height * 1.5;
        let last_f = p3(last);
        points.push(nan);
        points.push(last_f);
        points.push([
            last_f[0] + sign * len * hx,
            last_f[1] + sign * len * hy,
            last_f[2],
        ]);
    }

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(points),
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

// ── Grips ──────────────────────────────────────────────────────────────────

fn grips(leader: &Leader) -> Vec<GripDef> {
    let n = leader.vertices.len();
    let mut grips: Vec<GripDef> = leader
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| square_grip(i, glam::DVec3::new(v.x, v.y, v.z)))
        .collect();

    if n >= 2 {
        let sum = leader.vertices.iter().fold(glam::DVec3::ZERO, |acc, v| {
            acc + glam::DVec3::new(v.x, v.y, v.z)
        });
        grips.push(center_grip(n, sum / n as f64));
    }

    grips
}

fn apply_grip(leader: &mut Leader, grip_id: usize, apply: GripApply) {
    let n = leader.vertices.len();

    if grip_id < n {
        if n >= 3 && leader.creation_type == LeaderCreationType::WithText {
            // Grip del codo: mueve el codo libremente, pero arrastra también
            // el extremo del renglón manteniendo la distancia relativa.
            if grip_id == n - 2 {
                let old_elbow = leader.vertices[n - 2];
                let old_end = leader.vertices[n - 1];

                let delta = match apply {
                    GripApply::Absolute(p) => document::types::Vector3::new(
                        p.x - old_elbow.x,
                        p.y - old_elbow.y,
                        p.z - old_elbow.z,
                    ),
                    GripApply::Translate(d) => document::types::Vector3::new(d.x, d.y, d.z),
                };

                leader.vertices[n - 2].x = old_elbow.x + delta.x;
                leader.vertices[n - 2].y = old_elbow.y + delta.y;
                leader.vertices[n - 2].z = old_elbow.z + delta.z;

                leader.vertices[n - 1].x = old_end.x + delta.x;
                leader.vertices[n - 1].y = old_end.y + delta.y;
                leader.vertices[n - 1].z = old_end.z + delta.z;

                return;
            }

            // Grip del extremo horizontal: sólo debe estirar en X;
            // Y/Z quedan pegados al codo para que siga horizontal.
            if grip_id == n - 1 {
                let elbow = leader.vertices[n - 2];

                match apply {
                    GripApply::Absolute(p) => {
                        leader.vertices[n - 1].x = p.x;
                    }
                    GripApply::Translate(d) => {
                        leader.vertices[n - 1].x += d.x;
                    }
                }

                leader.vertices[n - 1].y = elbow.y;
                leader.vertices[n - 1].z = elbow.z;
                return;
            }
        }

        if let Some(v) = leader.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Absolute(p) => {
                    v.x = p.x;
                    v.y = p.y;
                    v.z = p.z;
                }
                GripApply::Translate(d) => {
                    v.x += d.x;
                    v.y += d.y;
                    v.z += d.z;
                }
            }
        }
    } else if let GripApply::Translate(d) = apply {
        leader.translate(document::types::Vector3::new(d.x, d.y, d.z));
    }
}

// ── Properties ─────────────────────────────────────────────────────────────

fn choice_prop(label: &str, field: &'static str, selected: &str, options: &[&str]) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: selected.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
        },
    }
}

/// Combined path/arrow "Type" value (path shape × arrowhead flag).
fn leader_type_str(path: &LeaderPathType, arrow: bool) -> &'static str {
    match (path, arrow) {
        (LeaderPathType::StraightLine, true) => "Line with arrow",
        (LeaderPathType::StraightLine, false) => "Line without arrow",
        (LeaderPathType::Spline, true) => "Spline with arrow",
        (LeaderPathType::Spline, false) => "Spline without arrow",
    }
}

fn properties(leader: &Leader) -> Vec<PropSection> {
    let n = leader.vertices.len();
    // The panel's Current Vertex focus, clamped to this leader's range.
    let vi = if n == 0 {
        0
    } else {
        crate::entities::dispatch::prop_current_vertex().min(n - 1)
    };
    let vertex_label = if n == 0 {
        "—".to_string()
    } else {
        (vi + 1).to_string()
    };

    // Geometry exposes one editable vertex at a time.
    let mut geometry = vec![stepper(
        t!("Current Vertex").as_ref(),
        "current_vertex",
        vertex_label,
    )];
    if let Some(v) = leader.vertices.get(vi) {
        geometry.push(edit(t!("Vertex X").as_ref(), "vertex_x", v.x));
        geometry.push(edit(t!("Vertex Y").as_ref(), "vertex_y", v.y));
        geometry.push(edit(t!("Vertex Z").as_ref(), "vertex_z", v.z));
    } else {
        geometry.push(ro(t!("Vertex X").as_ref(), "vertex_x", String::new()));
        geometry.push(ro(t!("Vertex Y").as_ref(), "vertex_y", String::new()));
        geometry.push(ro(t!("Vertex Z").as_ref(), "vertex_z", String::new()));
    }

    // The panel builder resolves the dimension style and annotative state.
    let misc = vec![
        Property {
            label: t!("Dim style").into_owned(),
            field: "dimension_style",
            value: PropValue::PlainText(leader.dimension_style.clone()),
        },
        choice_prop(
            t!("Type").as_ref(),
            "leader_type",
            leader_type_str(&leader.path_type, leader.arrow_enabled),
            &[
                "Line with arrow",
                "Line without arrow",
                "Spline with arrow",
                "Spline without arrow",
            ],
        ),
        ro(t!("Annotative").as_ref(), "annotative", "No"),
    ];

    // Lines & Arrows / Text / Fit are dimension-style-derived; the panel builder
    // resolves leader.dimension_style and fills these values from the DimStyle.
    let lines_arrows = vec![
        ro(t!("Arrow").as_ref(), "arrow_block", "Closed filled"),
        ro(t!("Arrow size").as_ref(), "arrow_size", String::new()),
        ro(t!("Dim line lineweight").as_ref(), "dim_line_lw", "ByLayer"),
        ro(t!("Dim line color").as_ref(), "dim_line_color", "ByLayer"),
    ];
    let text = vec![
        ro(t!("Text offset").as_ref(), "text_offset", String::new()),
        ro(t!("Text pos vert").as_ref(), "text_pos_vert", String::new()),
    ];
    let fit = vec![ro(
        t!("Dim scale overall").as_ref(),
        "dim_scale_overall",
        String::new(),
    )];

    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: geometry,
        },
        PropSection {
            title: t!("Misc").into_owned(),
            props: misc,
        },
        PropSection {
            title: t!("Lines & Arrows").into_owned(),
            props: lines_arrows,
        },
        PropSection {
            title: t!("Text").into_owned(),
            props: text,
        },
        PropSection {
            title: t!("Fit").into_owned(),
            props: fit,
        },
    ]
}

fn apply_geom_prop(leader: &mut Leader, field: &str, value: &str) {
    let f64 = |s: &str| -> Option<f64> { s.trim().parse().ok() };
    // Vertex X/Y/Z edit whichever vertex the Current Vertex navigator focuses.
    let vi = if leader.vertices.is_empty() {
        0
    } else {
        crate::entities::dispatch::prop_current_vertex().min(leader.vertices.len() - 1)
    };

    match field {
        "dimension_style" => leader.dimension_style = value.to_string(),
        "leader_type" => {
            let (p, a) = match value {
                "Line without arrow" => (LeaderPathType::StraightLine, false),
                "Spline with arrow" => (LeaderPathType::Spline, true),
                "Spline without arrow" => (LeaderPathType::Spline, false),
                _ => (LeaderPathType::StraightLine, true),
            };
            leader.path_type = p;
            leader.arrow_enabled = a;
        }
        "path_type" => {
            leader.path_type = match value {
                "Spline" => LeaderPathType::Spline,
                _ => LeaderPathType::StraightLine,
            };
        }
        "creation_type" => {
            leader.creation_type = match value {
                "With Tolerance" => LeaderCreationType::WithTolerance,
                "With Block" => LeaderCreationType::WithBlock,
                "No Annotation" => LeaderCreationType::NoAnnotation,
                _ => LeaderCreationType::WithText,
            };
        }
        "arrow_enabled" => {
            leader.arrow_enabled = if value == "toggle" {
                !leader.arrow_enabled
            } else {
                value == "true"
            }
        }
        "hookline_enabled" => {
            leader.hookline_enabled = if value == "toggle" {
                !leader.hookline_enabled
            } else {
                value == "true"
            }
        }
        "hookline_direction" => {
            leader.hookline_direction = match value {
                "Same" => HooklineDirection::Same,
                _ => HooklineDirection::Opposite,
            };
        }
        "text_height" => {
            if let Some(v) = f64(value) {
                leader.text_height = v;
            }
        }
        "text_width" => {
            if let Some(v) = f64(value) {
                leader.text_width = v;
            }
        }
        "normal_x" => {
            if let Some(v) = f64(value) {
                leader.normal.x = v;
            }
        }
        "normal_y" => {
            if let Some(v) = f64(value) {
                leader.normal.y = v;
            }
        }
        "normal_z" => {
            if let Some(v) = f64(value) {
                leader.normal.z = v;
            }
        }
        "h_dir_x" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.x = v;
            }
        }
        "h_dir_y" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.y = v;
            }
        }
        "h_dir_z" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.z = v;
            }
        }
        "block_offset_x" => {
            if let Some(v) = f64(value) {
                leader.block_offset.x = v;
            }
        }
        "block_offset_y" => {
            if let Some(v) = f64(value) {
                leader.block_offset.y = v;
            }
        }
        "block_offset_z" => {
            if let Some(v) = f64(value) {
                leader.block_offset.z = v;
            }
        }
        "ann_offset_x" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.x = v;
            }
        }
        "ann_offset_y" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.y = v;
            }
        }
        "ann_offset_z" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.z = v;
            }
        }
        "vertex_x" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(vi)) {
                vert.x = v;
            }
        }
        "vertex_y" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(vi)) {
                vert.y = v;
            }
        }
        "vertex_z" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(vi)) {
                vert.z = v;
            }
        }
        _ => {}
    }
}

// ── Transform ──────────────────────────────────────────────────────────────

fn apply_transform(leader: &mut Leader, t: &EntityTransform) {
    crate::entities::transform::apply_standard_entity_transform(leader, t, |entity, p1, p2| {
        for v in &mut entity.vertices {
            crate::entities::transform::reflect_xy_point(&mut v.x, &mut v.y, p1, p2);
        }
        // Offsets from the last vertex reflect as directions.
        let axis = p2 - p1;
        crate::entities::transform::reflect_xy_point(
            &mut entity.block_offset.x,
            &mut entity.block_offset.y,
            glam::DVec3::ZERO,
            axis,
        );
        crate::entities::transform::reflect_xy_point(
            &mut entity.annotation_offset.x,
            &mut entity.annotation_offset.y,
            glam::DVec3::ZERO,
            axis,
        );
        crate::entities::transform::reflect_xy_point(
            &mut entity.horizontal_direction.x,
            &mut entity.horizontal_direction.y,
            glam::DVec3::ZERO,
            axis,
        );
    });
}

// ── Trait impls ────────────────────────────────────────────────────────────

impl RenderConvertible for Leader {
    fn to_render(&self, _document: &document::Document) -> Option<RenderEntity> {
        if self.vertices.is_empty() {
            return None;
        }
        Some(to_render(self))
    }
}

impl crate::entities::traits::Grippable for Leader {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::entities::object::GripMenuItem> {
        use crate::entities::object::{GripMenuAction, GripMenuItem};
        let n = self.vertices.len();
        if grip_id == 0 {
            // Arrow head — stretch only.
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        } else if grip_id < n {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Add Vertex",
                    action: GripMenuAction::AddVertex,
                },
                GripMenuItem {
                    label: "Remove Vertex",
                    action: GripMenuAction::RemoveVertex,
                },
            ]
        } else {
            // Centroid grip — move whole leader.
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        }
    }
    fn apply_grip_menu(
        &mut self,
        grip_id: usize,
        action: crate::entities::object::GripMenuAction,
    ) {
        use crate::entities::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                let i1 = (grip_id + 1).min(n - 1);
                if i1 == grip_id {
                    return;
                }
                let v0 = &self.vertices[grip_id];
                let v1 = &self.vertices[i1];
                let mid = document::types::Vector3::new(
                    (v0.x + v1.x) * 0.5,
                    (v0.y + v1.y) * 0.5,
                    (v0.z + v1.z) * 0.5,
                );
                self.vertices.insert(i1, mid);
            }
            A::RemoveVertex if grip_id < n && n > 2 => {
                self.vertices.remove(grip_id);
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for Leader {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for Leader {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

