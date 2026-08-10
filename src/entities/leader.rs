use acadrust::entities::{HooklineDirection, Leader, LeaderCreationType, LeaderPathType};
use acadrust::Entity;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, ro_prop as ro, square_grip, stepper_prop as stepper,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::TangentGeom;
use crate::t;

// ── RenderConvertible (used for snap/grip key-vertices) ─────────────────────

/// Whether a leader draws a horizontal hookline. DWG stores no "hookline
/// exists" flag (only the direction bit), so the stored flag alone would drop
/// every hookline on DWG-loaded leaders: a text-annotated leader gets a
/// horizontal hook whenever its final segment leaves the horizontal by more
/// than 15°.
fn draws_hookline(leader: &Leader) -> bool {
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
    let p3 = |v: &acadrust::types::Vector3| -> [f64; 3] { [v.x, v.y, v.z] };
    let p3f = |v: &acadrust::types::Vector3| -> [f32; 3] { [v.x as f32, v.y as f32, v.z as f32] };

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
        if let Some(v) = leader.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Absolute(p) => {
                    v.x = p.x as f64;
                    v.y = p.y as f64;
                    v.z = p.z as f64;
                }
                GripApply::Translate(d) => {
                    v.x += d.x as f64;
                    v.y += d.y as f64;
                    v.z += d.z as f64;
                }
            }
        }
    } else if let GripApply::Translate(d) = apply {
        leader.translate(acadrust::types::Vector3::new(
            d.x as f64, d.y as f64, d.z as f64,
        ));
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
        crate::scene::view::dispatch::prop_current_vertex().min(n - 1)
    };
    let vertex_label = if n == 0 {
        "—".to_string()
    } else {
        format!("{} / {}", vi + 1, n)
    };

    // Geometry sits right after General — a leader owns editable path vertices,
    // navigated one at a time by the Current Vertex spinner.
    let mut geometry = vec![stepper(t!("Current Vertex").as_ref(), "current_vertex", vertex_label)];
    if let Some(v) = leader.vertices.get(vi) {
        geometry.push(edit(t!("Vertex X").as_ref(), "vertex_x", v.x));
        geometry.push(edit(t!("Vertex Y").as_ref(), "vertex_y", v.y));
        geometry.push(edit(t!("Vertex Z").as_ref(), "vertex_z", v.z));
    } else {
        geometry.push(ro(t!("Vertex X").as_ref(), "vertex_x", String::new()));
        geometry.push(ro(t!("Vertex Y").as_ref(), "vertex_y", String::new()));
        geometry.push(ro(t!("Vertex Z").as_ref(), "vertex_z", String::new()));
    }

    // Misc: Dim style (upgraded to a dropdown by the panel builder), the
    // combined path/arrow Type, and annotative state (from the dim style).
    let misc = vec![
        Property {
            label: t!("Dim style").into_owned(),
            field: "dimension_style",
            value: PropValue::EditText(leader.dimension_style.clone()),
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
    let fit = vec![ro(t!("Dim scale overall").as_ref(), "dim_scale_overall", String::new())];

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
        crate::scene::view::dispatch::prop_current_vertex().min(leader.vertices.len() - 1)
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
    crate::scene::view::transform::apply_standard_entity_transform(leader, t, |entity, p1, p2| {
        for v in &mut entity.vertices {
            crate::scene::view::transform::reflect_xy_point(&mut v.x, &mut v.y, p1, p2);
        }
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.block_offset.x,
            &mut entity.block_offset.y,
            p1,
            p2,
        );
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.annotation_offset.x,
            &mut entity.annotation_offset.y,
            p1,
            p2,
        );
    });
}

// ── Trait impls ────────────────────────────────────────────────────────────

impl RenderConvertible for Leader {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
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
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
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
    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                let i1 = (grip_id + 1).min(n - 1);
                if i1 == grip_id {
                    return;
                }
                let v0 = &self.vertices[grip_id];
                let v1 = &self.vertices[i1];
                let mid = acadrust::types::Vector3::new(
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

/// Per-entity tessellation entry for `Leader`. Lives here so all leader
/// tess code stays alongside the entity definition. Cross-entity dim
/// machinery (arrow shapes, `DimGeom`) lives in `scene::convert::tessellate` and
/// is reused via the dim arrow emitter so the leader matches the active
/// DIMSTYLE.
pub trait LeaderTess {
    fn tessellate(
        &self,
        document: &acadrust::CadDocument,
        handle: acadrust::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
    ) -> crate::scene::model::wire_model::WireModel;
}

impl LeaderTess for Leader {
    fn tessellate(
        &self,
        document: &acadrust::CadDocument,
        handle: acadrust::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
    ) -> crate::scene::model::wire_model::WireModel {
        use crate::scene::convert::tessellate::{append_arrow, arrow_from_block, ArrowKind, DimGeom};
        use crate::entities::dim_override as dov;
        use crate::scene::model::wire_model::WireModel;
        let xd = &self.common.extended_data;
        // Dim-line colour: a per-object ACAD_DSTYLE override (code 176, an ACI
        // index) wins over the assigned dim style's DIMCLRD; ByLayer / ByBlock
        // (0 / 256) and no setting fall through to the entity colour.
        let color = if selected {
            WireModel::SELECTED
        } else {
            let dim_clr = dov::int(xd, dov::DIMCLRD).or_else(|| {
                document
                    .dim_styles
                    .iter()
                    .find(|s| {
                        s.name.eq_ignore_ascii_case(&self.dimension_style)
                            || (self.dimension_style.trim().is_empty()
                                && s.name.eq_ignore_ascii_case("Standard"))
                    })
                    .map(|s| s.dimclrd)
            });
            match dim_clr {
                Some(idx) if idx != 0 && idx != 256 => crate::scene::convert::tess_util::aci_to_rgba(
                    &acadrust::types::Color::from_index(idx),
                ),
                _ => entity_color,
            }
        };
        // A concrete DIMLWD override sets the leader line's weight; ByLayer /
        // ByBlock / Default and no override keep the resolved weight passed in.
        let line_weight_px = match dov::int(xd, dov::DIMLWD) {
            Some(lwd) if lwd >= 0 => crate::scene::view::render::lineweight_to_px(
                &acadrust::types::LineWeight::from_value(lwd),
            ),
            _ => line_weight_px,
        };
        let name = handle.value().to_string();
        let p3 = |v: &acadrust::types::Vector3| -> [f32; 3] {
            [(v.x) as f32, (v.y) as f32, (v.z) as f32]
        };

        let verts = &self.vertices;

        if verts.len() < 2 {
            return WireModel {
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
                name,
                points: vec![],
                points_low: Vec::new(),
                color,
                selected,
                aci: 0,
                pattern_length: 0.0,
                pattern: [0.0; 8],
                line_weight_px,
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: vec![],
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                fill_tris: vec![],
                fill_tris_low: Vec::new(),
            };
        }

        // Build all geometry in absolute f64 and RTE-split at the end. At UTM
        // coordinates a raw `v.x as f32` quantises to ~0.5 units, which flattens
        // the oblique arrow tick into a horizontal line and jitters the path.
        let origin = [verts[0].x, verts[0].y, verts[0].z];
        let mut points: Vec<[f64; 3]> = verts.iter().map(|v| [v.x, v.y, v.z]).collect();
        let mut tangents: Vec<TangentGeom> = Vec::new();
        let key_vertices: Vec<[f64; 3]> = verts.iter().map(|v| [v.x, v.y, v.z]).collect();
        let mut fill_tris: Vec<[f64; 3]> = Vec::new();

        for i in 0..verts.len().saturating_sub(1) {
            tangents.push(TangentGeom::Line {
                p1: p3(&verts[i]),
                p2: p3(&verts[i + 1]),
            });
        }

        if self.arrow_enabled {
            // Resolve the active dim style → DIMLDRBLK to pick the arrow shape.
            // DIMASZ × DIMSCALE drives the size when available; otherwise fall
            // back to the legacy text-height heuristic.
            let style = document.dim_styles.iter().find(|s| {
                s.name.eq_ignore_ascii_case(&self.dimension_style)
                    || (self.dimension_style.trim().is_empty()
                        && s.name.eq_ignore_ascii_case("Standard"))
            });
            // Each of DIMSCALE / DIMASZ / DIMLDRBLK prefers a per-object override
            // over the style, so an edited leader arrow renders at its new size,
            // scale and shape.
            let dim_scale = dov::real(xd, dov::DIMSCALE)
                .filter(|v| *v > 1e-6)
                .or_else(|| style.map(|s| s.dimscale).filter(|v| *v > 1e-6))
                .unwrap_or(anno_scale as f64);
            let ovr_asz = dov::real(xd, dov::DIMASZ);
            let arrow_size = match (ovr_asz, style) {
                (Some(a), _) => (a * dim_scale) as f32,
                (None, Some(s)) => (s.dimasz * dim_scale) as f32,
                (None, None) => (self.text_height as f32).max(1.0) * anno_scale,
            };
            let arrow_blk = dov::handle(xd, dov::DIMLDRBLK).or_else(|| style.map(|s| s.dimldrblk));
            let arrow = match arrow_blk {
                Some(h) => arrow_from_block(document, h, arrow_size.max(0.001)),
                None => ArrowKind::Triangle {
                    size: arrow_size.max(0.001),
                    filled: true,
                    size_mul: 1.0,
                },
            };

            let tip = &verts[0];
            let next = &verts[1];
            let dx = (next.x - tip.x) as f32;
            let dy = (next.y - tip.y) as f32;
            let len = (dx * dx + dy * dy).sqrt().max(1e-9);
            let dir = Vec3::new(dx / len, dy / len, 0.0);
            // Omit the arrowhead when it is larger than the first leader segment
            // it would sit on: an oversized arrow on a short pointer is
            // suppressed, not drawn overshooting the segment. Without this an
            // annotation-scaled arrow (DIMASZ sized for a bigger plot) paints a
            // stray stroke longer than the leader itself.
            if arrow_size <= len {
                // Emit the arrow around a local origin (the tip) so
                // `append_arrow`'s f32 math stays precise, then lift each point
                // back to absolute f64. Reuse the dim arrow emitter so the
                // leader shape matches the DIMSTYLE in use (Closed Filled by
                // default, Dot, Tick, …).
                let mut arrow_geom = DimGeom::new();
                append_arrow(&mut arrow_geom, Vec3::ZERO, dir, &arrow);
                let lift = |p: &[f32; 3]| -> [f64; 3] {
                    [
                        p[0] as f64 + origin[0],
                        p[1] as f64 + origin[1],
                        p[2] as f64 + origin[2],
                    ]
                };
                if !arrow_geom.dim_lines.is_empty() {
                    points.push([f64::NAN; 3]);
                    points.extend(arrow_geom.dim_lines.iter().map(&lift));
                }
                fill_tris.extend(arrow_geom.arrow_fill.iter().map(&lift));
            }
        }

        if draws_hookline(self) {
            let last = verts.last().unwrap();
            let prev = &verts[verts.len() - 2];
            let sign = if (last.x - prev.x) >= 0.0 {
                1.0_f64
            } else {
                -1.0_f64
            };
            let style = document.dim_styles.iter().find(|s| {
                s.name.eq_ignore_ascii_case(&self.dimension_style)
                    || (self.dimension_style.trim().is_empty()
                        && s.name.eq_ignore_ascii_case("Standard"))
            });
            let dim_scale = dov::real(xd, dov::DIMSCALE)
                .filter(|v| *v > 1e-6)
                .or_else(|| style.map(|s| s.dimscale).filter(|v| *v > 1e-6))
                .unwrap_or(anno_scale as f64);
            // Hook length: DIMASZ, like the arrowhead, when a style resolves;
            // the legacy text-height heuristic otherwise.
            let mut land_len = match dov::real(xd, dov::DIMASZ)
                .or_else(|| style.map(|s| s.dimasz))
            {
                Some(a) => a * dim_scale,
                None => self.text_height * 1.5 * anno_scale as f64,
            };
            // With DIMTAD "text above", the annotation sits on top of the hook
            // and the hook underlines it: extend to the far edge of the MTEXT's
            // laid-out extents.
            let dimtad = dov::int(xd, dov::DIMTAD)
                .or_else(|| style.map(|s| s.dimtad))
                .unwrap_or(0);
            if dimtad != 0 {
                let mt = match document.get_entity(self.annotation_handle) {
                    Some(acadrust::entities::EntityType::MText(mt)) => {
                        Some((mt.extents_width, mt.insertion_point.x))
                    }
                    _ => None,
                };
                // Underline width: the annotation MTEXT's laid-out extents when
                // known, otherwise the leader's own stored annotation width
                // (DXF code 41) — a DXF-loaded MTEXT often has no computed
                // extents (extents_width == 0), which left the hook too short.
                let width = mt
                    .map(|(w, _)| w)
                    .filter(|w| *w > 0.0)
                    .unwrap_or(self.text_width);
                if width > 0.0 {
                    let ins_x = mt.map(|(_, x)| x).unwrap_or(last.x);
                    let to_far_edge = sign * (ins_x - last.x) + width;
                    land_len = land_len.max(to_far_edge);
                }
            }
            points.push([f64::NAN; 3]);
            points.push([last.x, last.y, last.z]);
            points.push([last.x + sign * land_len, last.y, last.z]);
        }

        // Split absolute f64 into the render/eye double-single so the GPU keeps
        // full precision at UTM coordinates (empty low bufs = absolute f32).
        let (points, points_low) = crate::scene::convert::tessellate::points_to_ds(points);
        let (fill_tris, fill_tris_low) =
            crate::scene::convert::tessellate::points_to_ds(fill_tris);

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
            name,
            points,
            points_low,
            color,
            selected,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px,
            snap_pts: vec![],
            tangent_geoms: tangents,
            key_vertices,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris,
            fill_tris_low,
        }
    }
}
