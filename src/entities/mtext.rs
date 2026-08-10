use acadrust::entities::{AttachmentPoint, DrawingDirection, MText};

use crate::command::EntityTransform;
use crate::entities::common::{
    edit_angle_prop as edit_angle, edit_prop as edit, num_prop as num_row, ro_prop as ro, square_grip, triangle_grip,
};
use crate::entities::text_support::{
    layout_mtext, resolve_text_style, GlyphBox, MTextColumns, MTextRenderOpts, MTextVAnchor,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::t;

/// The entity's `column_data` as the layout's column description.
///
/// `column_type` 0 means the MTEXT never opted into columns, so ignore whatever
/// stale width/count sit alongside it. Static (1) and dynamic (2) both flow at
/// the `\N` breaks in the value; the difference between them is how a *full*
/// column spills into the next, which the layout does not model.
fn columns_of(t: &MText) -> MTextColumns {
    let c = &t.column_data;
    if c.column_type == 0 {
        return MTextColumns::default();
    }
    MTextColumns {
        count: c.column_count.max(0) as usize,
        width: c.width as f32,
        gutter: c.gutter as f32,
    }
}

/// Combined attachment point shown as a single justify dropdown value.
fn attachment_str(a: &AttachmentPoint) -> &'static str {
    match a {
        AttachmentPoint::TopLeft => "Top left",
        AttachmentPoint::TopCenter => "Top center",
        AttachmentPoint::TopRight => "Top right",
        AttachmentPoint::MiddleLeft => "Middle left",
        AttachmentPoint::MiddleCenter => "Middle center",
        AttachmentPoint::MiddleRight => "Middle right",
        AttachmentPoint::BottomLeft => "Bottom left",
        AttachmentPoint::BottomCenter => "Bottom center",
        AttachmentPoint::BottomRight => "Bottom right",
    }
}

fn attachment_from_justify(value: &str) -> Option<AttachmentPoint> {
    Some(match value {
        "Top left" => AttachmentPoint::TopLeft,
        "Top center" => AttachmentPoint::TopCenter,
        "Top right" => AttachmentPoint::TopRight,
        "Middle left" => AttachmentPoint::MiddleLeft,
        "Middle center" => AttachmentPoint::MiddleCenter,
        "Middle right" => AttachmentPoint::MiddleRight,
        "Bottom left" => AttachmentPoint::BottomLeft,
        "Bottom center" => AttachmentPoint::BottomCenter,
        "Bottom right" => AttachmentPoint::BottomRight,
        _ => return None,
    })
}

fn drawing_dir_str(d: &DrawingDirection) -> &'static str {
    match d {
        DrawingDirection::LeftToRight => "Left to right",
        DrawingDirection::TopToBottom => "Top to bottom",
        DrawingDirection::ByStyle => "By style",
    }
}

/// Per-visible-character world-space boxes for the MText editor's
/// click-to-select preview. Uses the exact same layout opts as `to_render`
/// so the boxes line up with the rendered glyphs.
/// The MTEXT string to render: the live re-evaluated value when the entity
/// hosts a dynamic field, otherwise its stored (cached) value.
fn display_value(t: &MText, document: &acadrust::CadDocument) -> String {
    crate::entities::field::resolve(document, t.common.handle).unwrap_or_else(|| t.value.clone())
}

pub fn glyph_boxes(t: &MText, document: &acadrust::CadDocument) -> Vec<GlyphBox> {
    let resolved_style = resolve_text_style(&t.style, document);
    let attach_h_anchor: f32 = match t.attachment_point {
        AttachmentPoint::TopCenter
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::BottomCenter => 0.5,
        AttachmentPoint::TopRight | AttachmentPoint::MiddleRight | AttachmentPoint::BottomRight => {
            1.0
        }
        _ => 0.0,
    };
    let v_anchor = match t.attachment_point {
        AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => {
            MTextVAnchor::Top
        }
        AttachmentPoint::MiddleLeft
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::MiddleRight => MTextVAnchor::Middle,
        AttachmentPoint::BottomLeft
        | AttachmentPoint::BottomCenter
        | AttachmentPoint::BottomRight => MTextVAnchor::Bottom,
    };
    let rotation = if resolved_style.is_upside_down {
        t.rotation as f32 + std::f32::consts::PI
    } else {
        t.rotation as f32
    };
    let display = display_value(t, document);
    let layout = layout_mtext(&MTextRenderOpts {
        value: &display,
        insertion: [
            t.insertion_point.x,
            t.insertion_point.y,
            t.insertion_point.z,
        ],
        height: t.height as f32,
        rect_w: t.rectangle_width as f32,
        rotation,
        style: &resolved_style,
        attach_h_anchor,
        v_anchor,
        line_spacing_factor: t.line_spacing_factor as f32,
        vertical_text: matches!(t.drawing_direction, DrawingDirection::TopToBottom),
        want_glyph_boxes: true,
        columns: columns_of(t),
    });
    layout.glyph_boxes
}

fn to_render(t: &MText, document: &acadrust::CadDocument) -> RenderEntity {
    let resolved_style = resolve_text_style(&t.style, document);
    let attach_h_anchor: f32 = match t.attachment_point {
        AttachmentPoint::TopCenter
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::BottomCenter => 0.5,
        AttachmentPoint::TopRight | AttachmentPoint::MiddleRight | AttachmentPoint::BottomRight => {
            1.0
        }
        _ => 0.0,
    };
    let v_anchor = match t.attachment_point {
        AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => {
            MTextVAnchor::Top
        }
        AttachmentPoint::MiddleLeft
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::MiddleRight => MTextVAnchor::Middle,
        AttachmentPoint::BottomLeft
        | AttachmentPoint::BottomCenter
        | AttachmentPoint::BottomRight => MTextVAnchor::Bottom,
    };
    let rotation = if resolved_style.is_upside_down {
        t.rotation as f32 + std::f32::consts::PI
    } else {
        t.rotation as f32
    };
    let display = display_value(t, document);
    let layout = layout_mtext(&MTextRenderOpts {
        value: &display,
        insertion: [
            t.insertion_point.x,
            t.insertion_point.y,
            t.insertion_point.z,
        ],
        height: t.height as f32,
        rect_w: t.rectangle_width as f32,
        rotation,
        style: &resolved_style,
        attach_h_anchor,
        v_anchor,
        line_spacing_factor: t.line_spacing_factor as f32,
        vertical_text: matches!(t.drawing_direction, DrawingDirection::TopToBottom),
        want_glyph_boxes: false,
        columns: columns_of(t),
    });
    let insertion = glam::DVec3::new(
        t.insertion_point.x,
        t.insertion_point.y,
        t.insertion_point.z,
    );
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Text(layout.strokes),
        snap_pts: vec![(insertion, SnapHint::Insertion)],
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

/// The MTEXT's attachment anchors in the shared layout vocabulary.
fn attach_anchors(t: &MText) -> (f32, MTextVAnchor) {
    let h: f32 = match t.attachment_point {
        AttachmentPoint::TopCenter
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::BottomCenter => 0.5,
        AttachmentPoint::TopRight | AttachmentPoint::MiddleRight | AttachmentPoint::BottomRight => {
            1.0
        }
        _ => 0.0,
    };
    let v = match t.attachment_point {
        AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => {
            MTextVAnchor::Top
        }
        AttachmentPoint::MiddleLeft
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::MiddleRight => MTextVAnchor::Middle,
        AttachmentPoint::BottomLeft
        | AttachmentPoint::BottomCenter
        | AttachmentPoint::BottomRight => MTextVAnchor::Bottom,
    };
    (h, v)
}

/// Flow axis + attachment-side factor, from the shared text-grip helper.
fn width_grip_axis(t: &MText) -> (glam::DVec3, f64) {
    let (h, v) = attach_anchors(t);
    crate::entities::text_support::flow_grip_axis(
        t.rotation,
        matches!(t.drawing_direction, DrawingDirection::TopToBottom),
        h,
        v,
    )
}

fn grips(t: &MText) -> Vec<GripDef> {
    let p = glam::DVec3::new(
        t.insertion_point.x,
        t.insertion_point.y,
        t.insertion_point.z,
    );
    let (dir, k) = width_grip_axis(t);
    let width_grip = p + dir * (k * t.rectangle_width.max(0.0));
    vec![square_grip(0, p), triangle_grip(1, width_grip)]
}

fn columns_str(c: &acadrust::entities::MTextColumnData) -> &'static str {
    match c.column_type {
        1 => "Static",
        2 => "Dynamic",
        _ => "No columns",
    }
}

fn properties(t: &MText, text_style_names: &[String]) -> Vec<PropSection> {
    // Absolute line-space distance for single spacing (~1.66 * height) scaled
    // by the line-spacing factor.
    let line_space_distance = t.height * 1.666_666_666_666_667 * t.line_spacing_factor;
    let text_frame_on = (t.background_fill_flags & 0x10) != 0;
    // Defined width is only live without columns; defined height is live for
    // static columns or manual-height dynamic columns, grayed otherwise.
    let col_type = t.column_data.column_type;
    let width_editable = col_type == 0;
    let height_editable = col_type == 1 || (col_type == 2 && !t.column_data.auto_height);
    vec![
        PropSection {
            title: t!("Text").into_owned(),
            props: vec![
                Property {
                    label: t!("Contents").into_owned(),
                    field: "content",
                    value: PropValue::EditText(t.value.clone()),
                },
                Property {
                    label: t!("Style").into_owned(),
                    field: "style",
                    value: PropValue::Choice {
                        selected: if t.style.trim().is_empty() {
                            "Standard".into()
                        } else {
                            t.style.clone()
                        },
                        options: text_style_names.to_vec(),
                    },
                },
                ro(
                    t!("Annotative").as_ref(),
                    "annotative",
                    if t.is_annotative { "Yes" } else { "No" },
                ),
                Property {
                    label: t!("Justify").into_owned(),
                    field: "justify",
                    value: PropValue::Choice {
                        selected: attachment_str(&t.attachment_point).to_string(),
                        options: [
                            "Top left",
                            "Top center",
                            "Top right",
                            "Middle left",
                            "Middle center",
                            "Middle right",
                            "Bottom left",
                            "Bottom center",
                            "Bottom right",
                        ]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    },
                },
                Property {
                    label: t!("Direction").into_owned(),
                    field: "direction",
                    value: PropValue::Choice {
                        selected: drawing_dir_str(&t.drawing_direction).to_string(),
                        options: ["By style", "Left to right", "Top to bottom"]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    },
                },
                // Fixed by the style when the style says so — see Text.
                crate::entities::common::num_prop(
                    t!("Text height").as_ref(),
                    "height",
                    t.height,
                    crate::entities::common::style_fixed_height(&t.style).is_none(),
                ),
                edit_angle(t!("Rotation").as_ref(), "rotation", t.rotation.to_degrees()),
                edit(t!("Line space factor").as_ref(), "line_spacing", t.line_spacing_factor),
                edit(t!("Line space distance").as_ref(), "line_space_distance", line_space_distance),
                Property {
                    label: t!("Line space style").into_owned(),
                    field: "line_space_style",
                    value: PropValue::Choice {
                        selected: match t.line_spacing_style {
                            acadrust::entities::LineSpacingStyle::Exactly => "Exactly",
                            _ => "At least",
                        }
                        .to_string(),
                        options: ["At least", "Exactly"]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    },
                },
                Property {
                    label: t!("Background mask").into_owned(),
                    field: "background_mask",
                    value: PropValue::Choice {
                        selected: if t.background_fill_flags & 0x01 != 0 {
                            "Fill".into()
                        } else if t.background_fill_flags & 0x02 != 0 {
                            "Mask".into()
                        } else {
                            "Off".into()
                        },
                        options: ["Off", "Fill", "Mask"]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    },
                },
                num_row(t!("Defined width").as_ref(), "rect_w", t.rectangle_width, width_editable),
                num_row(
                    t!("Defined height").as_ref(),
                    "rect_h",
                    t.rectangle_height.unwrap_or(0.0),
                    height_editable,
                ),
                Property {
                    label: t!("Columns").into_owned(),
                    field: "columns",
                    value: PropValue::Choice {
                        selected: columns_str(&t.column_data).to_string(),
                        options: ["No columns", "Static", "Dynamic"]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    },
                },
                // Count / width / gutter are live only when columns are on.
                num_row(
                    t!("Column count").as_ref(),
                    "col_count",
                    t.column_data.column_count as f64,
                    col_type != 0,
                ),
                num_row(t!("Column width").as_ref(), "col_width", t.column_data.width, col_type != 0),
                num_row(
                    t!("Column gutter").as_ref(),
                    "col_gutter",
                    t.column_data.gutter,
                    col_type != 0,
                ),
                Property {
                    label: t!("Text frame").into_owned(),
                    field: "text_frame",
                    value: PropValue::BoolToggle {
                        field: "text_frame",
                        value: text_frame_on,
                    },
                },
            ],
        },
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Position X").as_ref(), "ins_x", t.insertion_point.x),
                edit(t!("Position Y").as_ref(), "ins_y", t.insertion_point.y),
                edit(t!("Position Z").as_ref(), "ins_z", t.insertion_point.z),
            ],
        },
    ]
}

fn apply_geom_prop(t: &mut MText, field: &str, value: &str) {
    match field {
        "content" => {
            t.value = value.to_string();
            return;
        }
        "style" => {
            t.style = value.to_string();
            return;
        }
        "justify" => {
            if let Some(next) = attachment_from_justify(value) {
                t.attachment_point = next;
            }
            return;
        }
        "background_mask" => {
            // Clear both fill bits, then set the chosen one. 0x01 = use the
            // background-fill colour, 0x02 = use the drawing-window colour (mask).
            t.background_fill_flags &= !0x03;
            match value {
                "Fill" => t.background_fill_flags |= 0x01,
                "Mask" => t.background_fill_flags |= 0x02,
                _ => {}
            }
            return;
        }
        "text_frame" => {
            // Rendered as a checkbox (BoolToggle) → the toggle sends "toggle";
            // flip the frame bit. Accept explicit "On"/"Off" too for safety.
            match value {
                "toggle" => t.background_fill_flags ^= 0x10,
                "On" => t.background_fill_flags |= 0x10,
                _ => t.background_fill_flags &= !0x10,
            }
            return;
        }
        "direction" => {
            t.drawing_direction = match value {
                "Left to right" => DrawingDirection::LeftToRight,
                "Top to bottom" => DrawingDirection::TopToBottom,
                "By style" => DrawingDirection::ByStyle,
                _ => return,
            };
            return;
        }
        "line_space_style" => {
            t.line_spacing_style = match value {
                "Exactly" => acadrust::entities::LineSpacingStyle::Exactly,
                "At least" => acadrust::entities::LineSpacingStyle::AtLeast,
                _ => return,
            };
            return;
        }
        "columns" => {
            let new_type = match value {
                "Static" => 1,
                "Dynamic" => 2,
                _ => 0,
            };
            // Compute defaults before borrowing column_data mutably.
            let default_w = if t.rectangle_width > 0.0 {
                t.rectangle_width
            } else {
                t.height * 10.0
            };
            let default_gut = t.height.max(1.0);
            let cd = &mut t.column_data;
            // Turning columns on from none seeds a consistent layout so the
            // stored data isn't a half-set column definition; the count / width
            // / gutter rows refine it. Dynamic columns default to auto-height.
            if new_type != 0 && cd.column_type == 0 {
                if cd.column_count < 2 {
                    cd.column_count = 2;
                }
                if cd.width <= 0.0 {
                    cd.width = default_w;
                }
                if cd.gutter <= 0.0 {
                    cd.gutter = default_gut;
                }
                cd.auto_height = new_type == 2;
            }
            cd.column_type = new_type;
            return;
        }
        _ => {}
    }
    let Some(v) = crate::entities::common::parse_f64(value) else {
        return;
    };
    match field {
        "ins_x" => t.insertion_point.x = v,
        "ins_y" => t.insertion_point.y = v,
        "ins_z" => t.insertion_point.z = v,
        "height" if v > 0.0 => t.height = v,
        "rect_w" if v > 0.0 => t.rectangle_width = v,
        "rect_h" if v > 0.0 => t.rectangle_height = Some(v),
        "rotation" => t.rotation = v.to_radians(),
        "line_spacing" if v > 0.0 => t.line_spacing_factor = v,
        // Editing the absolute distance back-solves the line-spacing factor so
        // the two stay consistent (distance = height × 5/3 × factor).
        "line_space_distance" if v > 0.0 => {
            let denom = t.height * 1.666_666_666_666_667;
            if denom > 0.0 {
                t.line_spacing_factor = v / denom;
            }
        }
        "col_count" if v >= 1.0 => t.column_data.column_count = v.round() as i32,
        "col_width" if v > 0.0 => t.column_data.width = v,
        "col_gutter" if v >= 0.0 => t.column_data.gutter = v,
        _ => {}
    }
}

fn apply_grip(t: &mut MText, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            t.insertion_point.x = p.x as f64;
            t.insertion_point.y = p.y as f64;
            t.insertion_point.z = p.z as f64;
        }
        (0, GripApply::Translate(d)) => {
            t.insertion_point.x += d.x as f64;
            t.insertion_point.y += d.y as f64;
            t.insertion_point.z += d.z as f64;
        }
        (1, GripApply::Absolute(p)) => {
            // Project onto the flow axis the grip is drawn on, undoing the
            // attachment-side factor so dragging the grip on either side of
            // the insertion widens the same box.
            let (dir, k) = width_grip_axis(t);
            let dx = p.x as f64 - t.insertion_point.x;
            let dy = p.y as f64 - t.insertion_point.y;
            let projected = dx * dir.x + dy * dir.y;
            t.rectangle_width = (projected / k).max(0.01);
        }
        _ => {}
    }
}

fn apply_transform(t: &mut MText, tr: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(t, tr, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.insertion_point.x,
            &mut entity.insertion_point.y,
            p1,
            p2,
        );
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let line_angle = dy.atan2(dx);
        entity.rotation = 2.0 * line_angle - entity.rotation;
    });
}

impl RenderConvertible for MText {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self, document))
    }
}

impl Grippable for MText {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }

    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        if grip_id == 0 {
            // Insertion point
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Move with Text",
                    action: GripMenuAction::MoveWithText,
                },
                GripMenuItem {
                    label: "Rotate",
                    action: GripMenuAction::RotateText,
                },
            ]
        } else {
            // Width grip
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        }
    }

    fn apply_grip_menu(&mut self, _grip_id: usize, _action: crate::scene::model::object::GripMenuAction) {
        // Rotate needs a follow-up angle handled by
        // `apply_grip_menu_value`; Move-with-Text is the default drag.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::RotateText => Some("Rotation (deg)"),
            _ => None,
        }
    }

    fn apply_grip_menu_value(
        &mut self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if matches!(action, A::RotateText) {
            self.rotation = value.to_radians();
        }
    }
}

impl PropertyEditable for MText {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        properties(self, text_style_names)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for MText {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

impl crate::entities::traits::TextContent for acadrust::entities::MText {
    fn text_content(&self) -> Option<String> {
        Some(self.value.clone())
    }
    fn replace_text(&mut self, search: &str, rep: &str) {
        let search_lc = search.to_lowercase();
        if self.value.to_lowercase().contains(&search_lc) {
            self.value = self.value.replace(search, rep);
        }
    }
}
