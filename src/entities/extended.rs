use acadrust::entities::{
    ArcAlignedTextData, ExtendedEntity, ExtendedEntityData, GeoPositionMarkerData,
    PointCloudData, PointCloudExData, RemoteTextData, SectionObjectData,
};
use acadrust::types::{Handle, Transform, Vector3};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_angle_prop, edit_prop, parse_f64, ro_prop, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{
    GripApply, GripDef, PropSection, PropValue, Property,
};
use crate::scene::model::wire_model::SnapHint;

const NAN: [f64; 3] = [f64::NAN; 3];

fn vector_text(value: Vector3) -> String {
    format!("{:.6}, {:.6}, {:.6}", value.x, value.y, value.z)
}

fn handle_text(handle: Handle) -> String {
    if handle.is_null() {
        "None".to_string()
    } else {
        format!("{:X}", handle.value())
    }
}

fn text_prop(label: &str, field: &'static str, value: &str) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::PlainText(value.to_string()),
    }
}

fn bool_prop(label: &str, field: &'static str, value: bool) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::BoolToggle { field, value },
    }
}

fn push_segment(points: &mut Vec<[f64; 3]>, first: [f64; 3], second: [f64; 3]) {
    if !points.is_empty() {
        points.push(NAN);
    }
    points.push(first);
    points.push(second);
}

fn push_chain(points: &mut Vec<[f64; 3]>, chain: impl IntoIterator<Item = [f64; 3]>) {
    let chain: Vec<_> = chain.into_iter().collect();
    if chain.len() < 2 {
        return;
    }
    if !points.is_empty() {
        points.push(NAN);
    }
    points.extend(chain);
}

fn push_box(points: &mut Vec<[f64; 3]>, min: Vector3, max: Vector3) {
    let corners = [
        [min.x, min.y, min.z],
        [max.x, min.y, min.z],
        [max.x, max.y, min.z],
        [min.x, max.y, min.z],
        [min.x, min.y, max.z],
        [max.x, min.y, max.z],
        [max.x, max.y, max.z],
        [min.x, max.y, max.z],
    ];
    for (a, b) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        push_segment(points, corners[a], corners[b]);
    }
}

fn normalized(value: Vector3, fallback: Vector3) -> Vector3 {
    let length = value.length();
    if length > 1e-12 {
        value / length
    } else {
        fallback
    }
}

fn plane_axes(normal: Vector3) -> (Vector3, Vector3, Vector3) {
    let normal = normalized(normal, Vector3::UNIT_Z);
    let (x, y) =
        crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    (
        Vector3::new(x.0, x.1, x.2),
        Vector3::new(y.0, y.1, y.2),
        normal,
    )
}

fn add_scaled(origin: Vector3, x: Vector3, sx: f64, y: Vector3, sy: f64) -> [f64; 3] {
    [
        origin.x + x.x * sx + y.x * sy,
        origin.y + x.y * sx + y.y * sy,
        origin.z + x.z * sx + y.z * sy,
    ]
}

fn append_planar_text(
    points: &mut Vec<[f64; 3]>,
    text: &str,
    font: &str,
    origin: Vector3,
    normal: Vector3,
    rotation: f64,
    height: f64,
    width_factor: f64,
) {
    if text.is_empty() || height.abs() < 1e-12 {
        return;
    }
    let (axis_x, axis_y, _) = plane_axes(normal);
    let (cos, sin) = rotation.sin_cos();
    let baseline = axis_x * cos + axis_y * sin;
    let upward = axis_y * cos - axis_x * sin;
    let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
        [0.0, 0.0],
        height.abs() as f32,
        0.0,
        width_factor as f32,
        0.0,
        if font.trim().is_empty() { "standard" } else { font },
        text,
    );
    for stroke in strokes {
        push_chain(
            points,
            stroke
                .into_iter()
                .map(|[x, y]| add_scaled(origin, baseline, x as f64, upward, y as f64)),
        );
    }
}

fn append_arc_aligned_text(points: &mut Vec<[f64; 3]>, data: &ArcAlignedTextData) {
    if data.text.is_empty() || data.radius.abs() < 1e-12 || data.text_size.abs() < 1e-12 {
        return;
    }
    let (axis_x, axis_y, _) = plane_axes(data.normal);
    let direction = if data.reverse || data.text_direction < 0 {
        -1.0
    } else {
        1.0
    };
    let mut angle = if direction > 0.0 {
        data.start_angle
    } else {
        data.end_angle
    };
    let advance = data.text_size.abs()
        * data.x_scale.abs().max(0.01)
        * data.character_spacing.abs().max(0.01)
        * 0.72;
    let delta = direction * advance / data.radius.abs().max(1e-9);
    let radius = data.radius + data.offset_from_arc;
    let font = if data.font_name.trim().is_empty() {
        data.style_name.as_str()
    } else {
        data.font_name.as_str()
    };
    for character in data.text.chars() {
        let radial = axis_x * angle.cos() + axis_y * angle.sin();
        let tangent = (axis_y * angle.cos() - axis_x * angle.sin()) * direction;
        let origin = data.center + radial * radius;
        let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
            [0.0, 0.0],
            data.text_size.abs() as f32,
            0.0,
            data.x_scale.abs().max(0.01) as f32,
            0.0,
            if font.trim().is_empty() { "standard" } else { font },
            &character.to_string(),
        );
        for stroke in strokes {
            push_chain(
                points,
                stroke.into_iter().map(|[x, y]| {
                    add_scaled(origin, tangent, x as f64, radial, y as f64)
                }),
            );
        }
        angle += delta;
    }
}

fn section_lines(data: &SectionObjectData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    push_chain(
        &mut points,
        data.vertices.iter().map(|p| [p.x, p.y, p.z]),
    );
    push_chain(
        &mut points,
        data.back_line_vertices.iter().map(|p| [p.x, p.y, p.z]),
    );
    points
}

fn remote_text_lines(data: &RemoteTextData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    append_planar_text(
        &mut points,
        &data.text,
        &data.style_name,
        data.position,
        data.normal,
        data.rotation,
        data.height,
        1.0,
    );
    points
}

fn geo_marker_lines(data: &GeoPositionMarkerData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    let radius = data.radius.abs().max(1e-6);
    let count = 32;
    push_chain(
        &mut points,
        (0..=count).map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / count as f64;
            [
                data.position.x + radius * angle.cos(),
                data.position.y + radius * angle.sin(),
                data.position.z,
            ]
        }),
    );
    push_segment(
        &mut points,
        [data.position.x - radius, data.position.y, data.position.z],
        [data.position.x + radius, data.position.y, data.position.z],
    );
    push_segment(
        &mut points,
        [data.position.x, data.position.y - radius, data.position.z],
        [data.position.x, data.position.y + radius, data.position.z],
    );
    let label = data
        .embedded_mtext
        .as_ref()
        .filter(|_| data.mtext_visible)
        .map(|text| text.value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(data.notes.as_str());
    if !label.trim().is_empty() {
        let origin = Vector3::new(
            data.position.x + radius + data.landing_gap.max(0.0),
            data.position.y,
            data.position.z,
        );
        push_segment(
            &mut points,
            [
                data.position.x + radius,
                data.position.y,
                data.position.z,
            ],
            [origin.x, origin.y, origin.z],
        );
        append_planar_text(
            &mut points,
            label,
            "standard",
            origin,
            Vector3::UNIT_Z,
            0.0,
            radius * 0.8,
            1.0,
        );
    }
    points
}

fn point_cloud_lines(data: &PointCloudData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    push_box(&mut points, data.extents_min, data.extents_max);
    points
}

fn point_cloud_clip_lines(data: &PointCloudData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    if data.show_clipping {
        for clip in &data.clippings {
            if clip.vertices.len() < 2 {
                continue;
            }
            for z in [clip.z_min, clip.z_max] {
                let mut chain: Vec<[f64; 3]> = clip
                    .vertices
                    .iter()
                    .map(|point| [point.x, point.y, z])
                    .collect();
                chain.push(chain[0]);
                push_chain(&mut points, chain);
            }
        }
    }
    points
}

fn point_cloud_ex_lines(data: &PointCloudExData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    push_box(&mut points, data.extents_min, data.extents_max);
    points
}

fn point_cloud_ex_clip_lines(data: &PointCloudExData) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    if data.show_cropping {
        for crop in &data.croppings {
            if crop.points.len() < 2 {
                continue;
            }
            let mut chain: Vec<[f64; 3]> =
                crop.points.iter().map(|p| [p.x, p.y, p.z]).collect();
            if crop.points.len() > 2 {
                chain.push(chain[0]);
            }
            push_chain(&mut points, chain);
        }
    }
    points
}

pub(crate) fn point_cloud_frame_lines(entity: &ExtendedEntity) -> Option<Vec<[f64; 3]>> {
    match &entity.data {
        ExtendedEntityData::PointCloud(data) => Some(point_cloud_clip_lines(data)),
        ExtendedEntityData::PointCloudEx(data) => Some(point_cloud_ex_clip_lines(data)),
        _ => None,
    }
}

fn camera_lines(document: &acadrust::CadDocument, view_handle: Handle) -> Vec<[f64; 3]> {
    let Some(view) = document.views.iter().find(|view| view.handle == view_handle) else {
        return Vec::new();
    };
    let direction = normalized(view.direction, Vector3::UNIT_Z);
    let distance = view.direction.length().max(view.height.abs()).max(1.0);
    let eye = view.target + direction * distance;
    let (right, up, _) = plane_axes(direction);
    let half_width = view.width.abs().max(view.height.abs()) * 0.08;
    let half_height = view.height.abs().max(view.width.abs()) * 0.06;
    let corners = [
        view.target + right * half_width + up * half_height,
        view.target - right * half_width + up * half_height,
        view.target - right * half_width - up * half_height,
        view.target + right * half_width - up * half_height,
    ];
    let mut points = Vec::new();
    for corner in corners {
        push_segment(
            &mut points,
            [eye.x, eye.y, eye.z],
            [corner.x, corner.y, corner.z],
        );
    }
    push_chain(
        &mut points,
        corners
            .into_iter()
            .chain(std::iter::once(corners[0]))
            .map(|p| [p.x, p.y, p.z]),
    );
    points
}

fn to_render(entity: &ExtendedEntity, document: &acadrust::CadDocument) -> Option<RenderEntity> {
    let (points, snaps, keys): (
        Vec<[f64; 3]>,
        Vec<(glam::DVec3, SnapHint)>,
        Vec<[f64; 3]>,
    ) = match &entity.data {
        ExtendedEntityData::Camera { view_handle } => {
            let points = camera_lines(document, *view_handle);
            (points, Vec::new(), Vec::new())
        }
        ExtendedEntityData::SectionObject(data) => {
            let snaps = data
                .vertices
                .iter()
                .chain(data.back_line_vertices.iter())
                .map(|point| {
                    (
                        glam::DVec3::new(point.x, point.y, point.z),
                        SnapHint::Node,
                    )
                })
                .collect();
            let keys = data
                .vertices
                .iter()
                .chain(data.back_line_vertices.iter())
                .map(|point| [point.x, point.y, point.z])
                .collect();
            (section_lines(data), snaps, keys)
        }
        ExtendedEntityData::ArcAlignedText(data) => {
            let mut points = Vec::new();
            append_arc_aligned_text(&mut points, data);
            (
                points,
                vec![(
                    glam::DVec3::new(data.center.x, data.center.y, data.center.z),
                    SnapHint::Center,
                )],
                vec![[data.center.x, data.center.y, data.center.z]],
            )
        }
        ExtendedEntityData::RemoteText(data) => (
            remote_text_lines(data),
            vec![(
                glam::DVec3::new(data.position.x, data.position.y, data.position.z),
                SnapHint::Insertion,
            )],
            vec![[data.position.x, data.position.y, data.position.z]],
        ),
        ExtendedEntityData::GeoPositionMarker(data) => (
            geo_marker_lines(data),
            vec![(
                glam::DVec3::new(data.position.x, data.position.y, data.position.z),
                SnapHint::Node,
            )],
            vec![[data.position.x, data.position.y, data.position.z]],
        ),
        ExtendedEntityData::PointCloud(data) => (
            point_cloud_lines(data),
            vec![(
                glam::DVec3::new(data.origin.x, data.origin.y, data.origin.z),
                SnapHint::Insertion,
            )],
            vec![
                [data.extents_min.x, data.extents_min.y, data.extents_min.z],
                [data.extents_max.x, data.extents_max.y, data.extents_max.z],
            ],
        ),
        ExtendedEntityData::PointCloudEx(data) => (
            point_cloud_ex_lines(data),
            Vec::new(),
            vec![
                [data.extents_min.x, data.extents_min.y, data.extents_min.z],
                [data.extents_max.x, data.extents_max.y, data.extents_max.z],
            ],
        ),
        _ => return None,
    };
    if points.len() < 2 {
        return None;
    }
    Some(RenderEntity {
        object: RenderObject::Lines(points),
        snap_pts: snaps,
        tangent_geoms: Vec::new(),
        key_vertices: keys,
        fill_tris: Vec::new(),
        pick_tris: Vec::new(),
    })
}

fn section_properties(data: &SectionObjectData) -> Vec<PropSection> {
    let vertices = data
        .vertices
        .iter()
        .enumerate()
        .map(|(index, point)| format!("{}: {}", index + 1, vector_text(*point)))
        .collect::<Vec<_>>()
        .join("\n");
    let back_vertices = data
        .back_line_vertices
        .iter()
        .enumerate()
        .map(|(index, point)| format!("{}: {}", index + 1, vector_text(*point)))
        .collect::<Vec<_>>()
        .join("\n");
    vec![
        PropSection {
            title: t!("Section Object").into_owned(),
            props: vec![
                text_prop(t!("Name").as_ref(), "ext_section_name", &data.name),
                ro_prop(t!("State").as_ref(), "ext_section_state", data.state.to_string()),
                ro_prop(t!("Flags").as_ref(), "ext_section_flags", data.flags.to_string()),
                ro_prop(t!("Vertical Direction").as_ref(),
                    "ext_section_vertical",
                    vector_text(data.vertical_direction),
                ),
                edit_prop(t!("Top Height").as_ref(), "ext_section_top", data.top_height),
                edit_prop(t!("Bottom Height").as_ref(), "ext_section_bottom", data.bottom_height),
                ro_prop(t!("Indicator Alpha").as_ref(),
                    "ext_section_alpha",
                    data.indicator_alpha.to_string(),
                ),
                ro_prop(t!("Indicator Color").as_ref(),
                    "ext_section_color",
                    format!("{:?}", data.indicator_color),
                ),
                ro_prop(t!("Settings").as_ref(),
                    "ext_section_settings",
                    handle_text(data.settings_handle),
                ),
            ],
        },
        PropSection {
            title: t!("Section Vertices").into_owned(),
            props: vec![
                ro_prop(t!("Cutting Line").as_ref(), "ext_section_vertices", vertices),
                ro_prop(t!("Back Line").as_ref(), "ext_section_back_vertices", back_vertices),
            ],
        },
    ]
}

fn arc_text_properties(data: &ArcAlignedTextData) -> Vec<PropSection> {
    vec![PropSection {
        title: t!("Arc-Aligned Text").into_owned(),
        props: vec![
            text_prop(t!("Text").as_ref(), "ext_arc_text", &data.text),
            text_prop(t!("Font").as_ref(), "ext_arc_font", &data.font_name),
            text_prop(t!("Big Font").as_ref(), "ext_arc_big_font", &data.big_font_name),
            text_prop(t!("Style").as_ref(), "ext_arc_style", &data.style_name),
            ro_prop(t!("Center").as_ref(), "ext_arc_center", vector_text(data.center)),
            edit_prop(t!("Radius").as_ref(), "ext_arc_radius", data.radius),
            edit_prop(t!("X Scale").as_ref(), "ext_arc_xscale", data.x_scale),
            edit_prop(t!("Text Size").as_ref(), "ext_arc_size", data.text_size),
            edit_prop(t!("Character Spacing").as_ref(),
                "ext_arc_spacing",
                data.character_spacing,
            ),
            edit_prop(t!("Offset From Arc").as_ref(),
                "ext_arc_offset",
                data.offset_from_arc,
            ),
            edit_prop(t!("Right Offset").as_ref(), "ext_arc_right", data.right_offset),
            edit_prop(t!("Left Offset").as_ref(), "ext_arc_left", data.left_offset),
            edit_angle_prop(t!("Start Angle").as_ref(),
                "ext_arc_start",
                data.start_angle.to_degrees(),
            ),
            edit_angle_prop(t!("End Angle").as_ref(), "ext_arc_end", data.end_angle.to_degrees()),
            bool_prop(t!("Reverse").as_ref(), "ext_arc_reverse", data.reverse),
            ro_prop(t!("Text Direction").as_ref(),
                "ext_arc_direction",
                data.text_direction.to_string(),
            ),
            ro_prop(t!("Alignment").as_ref(), "ext_arc_alignment", data.alignment.to_string()),
            ro_prop(t!("Text Position").as_ref(),
                "ext_arc_position",
                data.text_position.to_string(),
            ),
            bool_prop(t!("Bold").as_ref(), "ext_arc_bold", data.bold),
            bool_prop(t!("Italic").as_ref(), "ext_arc_italic", data.italic),
            bool_prop(t!("Underlined").as_ref(), "ext_arc_underlined", data.underlined),
            ro_prop(t!("Character Set").as_ref(),
                "ext_arc_charset",
                data.character_set.to_string(),
            ),
            ro_prop(t!("Pitch And Family").as_ref(),
                "ext_arc_pitch",
                data.pitch_and_family.to_string(),
            ),
            bool_prop(t!("SHX").as_ref(), "ext_arc_shx", data.is_shx),
            ro_prop(t!("Text Color").as_ref(), "ext_arc_color", data.text_color.to_string()),
            ro_prop(t!("Normal").as_ref(), "ext_arc_normal", vector_text(data.normal)),
            bool_prop(t!("Wizard Flag").as_ref(), "ext_arc_wizard", data.wizard_flag),
            ro_prop(t!("Arc").as_ref(), "ext_arc_handle", handle_text(data.arc_handle)),
        ],
    }]
}

fn remote_text_properties(data: &RemoteTextData) -> Vec<PropSection> {
    vec![PropSection {
        title: t!("Remote Text").into_owned(),
        props: vec![
            text_prop(t!("Text").as_ref(), "ext_rtext_text", &data.text),
            ro_prop(t!("Position").as_ref(), "ext_rtext_position", vector_text(data.position)),
            ro_prop(t!("Normal").as_ref(), "ext_rtext_normal", vector_text(data.normal)),
            edit_angle_prop(t!("Rotation").as_ref(),
                "ext_rtext_rotation",
                data.rotation.to_degrees(),
            ),
            edit_prop(t!("Height").as_ref(), "ext_rtext_height", data.height),
            text_prop(t!("Style Name").as_ref(), "ext_rtext_style", &data.style_name),
            ro_prop(t!("Style Handle").as_ref(),
                "ext_rtext_style_handle",
                handle_text(data.style_handle),
            ),
            ro_prop(t!("Flags").as_ref(), "ext_rtext_flags", data.flags.to_string()),
        ],
    }]
}

fn geo_marker_properties(data: &GeoPositionMarkerData) -> Vec<PropSection> {
    vec![PropSection {
        title: t!("Geographic Position Marker").into_owned(),
        props: vec![
            ro_prop(t!("Class Version").as_ref(),
                "ext_geo_version",
                data.class_version.to_string(),
            ),
            ro_prop(t!("Position").as_ref(), "ext_geo_position", vector_text(data.position)),
            edit_prop(t!("Radius").as_ref(), "ext_geo_radius", data.radius),
            text_prop(t!("Notes").as_ref(), "ext_geo_notes", &data.notes),
            edit_prop(t!("Landing Gap").as_ref(), "ext_geo_gap", data.landing_gap),
            bool_prop(t!("Text Visible").as_ref(), "ext_geo_visible", data.mtext_visible),
            ro_prop(t!("Text Alignment").as_ref(),
                "ext_geo_alignment",
                data.text_alignment.to_string(),
            ),
            bool_prop(t!("Frame Text").as_ref(), "ext_geo_frame", data.enable_frame_text),
            ro_prop(t!("Embedded MText").as_ref(),
                "ext_geo_mtext",
                data.embedded_mtext
                    .as_ref()
                    .map(|text| text.value.clone())
                    .unwrap_or_default(),
            ),
        ],
    }]
}

fn point_cloud_properties(data: &PointCloudData) -> Vec<PropSection> {
    let clips = data
        .clippings
        .iter()
        .enumerate()
        .map(|(index, clip)| {
            format!(
                "{}: type {}; inverted {}; vertices {}; Z {:.6}..{:.6}",
                index + 1,
                clip.clip_type,
                clip.inverted,
                clip.vertices.len(),
                clip.z_min,
                clip.z_max
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    vec![
        PropSection {
            title: t!("Point Cloud").into_owned(),
            props: vec![
                ro_prop(t!("Class Version").as_ref(),
                    "ext_pc_version",
                    data.class_version.to_string(),
                ),
                ro_prop(t!("Origin").as_ref(), "ext_pc_origin", vector_text(data.origin)),
                text_prop(t!("Saved File").as_ref(), "ext_pc_file", &data.saved_filename),
                ro_prop(t!("Source Files").as_ref(),
                    "ext_pc_sources",
                    data.source_files.join("\n"),
                ),
                ro_prop(t!("Extents Min").as_ref(), "ext_pc_min", vector_text(data.extents_min)),
                ro_prop(t!("Extents Max").as_ref(), "ext_pc_max", vector_text(data.extents_max)),
                ro_prop(t!("Point Count").as_ref(),
                    "ext_pc_count",
                    data.point_count.to_string(),
                ),
                text_prop(t!("UCS Name").as_ref(), "ext_pc_ucs", &data.ucs_name),
                ro_prop(t!("UCS Origin").as_ref(),
                    "ext_pc_ucs_origin",
                    vector_text(data.ucs_origin),
                ),
                ro_prop(t!("UCS X").as_ref(),
                    "ext_pc_ucs_x",
                    vector_text(data.ucs_x_direction),
                ),
                ro_prop(t!("UCS Y").as_ref(),
                    "ext_pc_ucs_y",
                    vector_text(data.ucs_y_direction),
                ),
                ro_prop(t!("UCS Z").as_ref(),
                    "ext_pc_ucs_z",
                    vector_text(data.ucs_z_direction),
                ),
                ro_prop(t!("Definition").as_ref(),
                    "ext_pc_definition",
                    handle_text(data.definition_handle),
                ),
                ro_prop(t!("Reactor").as_ref(),
                    "ext_pc_reactor",
                    handle_text(data.reactor_handle),
                ),
            ],
        },
        PropSection {
            title: t!("Point Cloud Display").into_owned(),
            props: vec![
                bool_prop(t!("Show Intensity").as_ref(), "ext_pc_show_intensity", data.show_intensity),
                ro_prop(t!("Intensity Scheme").as_ref(),
                    "ext_pc_intensity_scheme",
                    data.intensity_scheme.to_string(),
                ),
                edit_prop(t!("Minimum Intensity").as_ref(),
                    "ext_pc_intensity_min",
                    data.minimum_intensity,
                ),
                edit_prop(t!("Maximum Intensity").as_ref(),
                    "ext_pc_intensity_max",
                    data.maximum_intensity,
                ),
                edit_prop(t!("Low Threshold").as_ref(),
                    "ext_pc_low_threshold",
                    data.low_intensity_threshold,
                ),
                edit_prop(t!("High Threshold").as_ref(),
                    "ext_pc_high_threshold",
                    data.high_intensity_threshold,
                ),
                bool_prop(t!("Show Clipping").as_ref(), "ext_pc_show_clipping", data.show_clipping),
                ro_prop(t!("Clippings").as_ref(), "ext_pc_clippings", clips),
            ],
        },
    ]
}

fn point_cloud_ex_properties(data: &PointCloudExData) -> Vec<PropSection> {
    let crops = data
        .croppings
        .iter()
        .enumerate()
        .map(|(index, crop)| {
            format!(
                "{}: type {}; inside {}; inverted {}; points {}; plane [{}]",
                index + 1,
                crop.crop_type,
                crop.inside,
                crop.inverted,
                crop.points.len(),
                vector_text(crop.plane)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    vec![
        PropSection {
            title: t!("Point Cloud Ex").into_owned(),
            props: vec![
                ro_prop(t!("Class Version").as_ref(),
                    "ext_pcx_version",
                    data.class_version.to_string(),
                ),
                text_prop(t!("Name").as_ref(), "ext_pcx_name", &data.name),
                ro_prop(t!("Extents Min").as_ref(), "ext_pcx_min", vector_text(data.extents_min)),
                ro_prop(t!("Extents Max").as_ref(), "ext_pcx_max", vector_text(data.extents_max)),
                ro_prop(t!("UCS Origin").as_ref(),
                    "ext_pcx_ucs_origin",
                    vector_text(data.ucs_origin),
                ),
                ro_prop(t!("UCS X").as_ref(),
                    "ext_pcx_ucs_x",
                    vector_text(data.ucs_x_direction),
                ),
                ro_prop(t!("UCS Y").as_ref(),
                    "ext_pcx_ucs_y",
                    vector_text(data.ucs_y_direction),
                ),
                ro_prop(t!("UCS Z").as_ref(),
                    "ext_pcx_ucs_z",
                    vector_text(data.ucs_z_direction),
                ),
                bool_prop(t!("Locked").as_ref(), "ext_pcx_locked", data.locked),
                ro_prop(t!("Definition").as_ref(),
                    "ext_pcx_definition",
                    handle_text(data.definition_handle),
                ),
                ro_prop(t!("Reactor").as_ref(),
                    "ext_pcx_reactor",
                    handle_text(data.reactor_handle),
                ),
            ],
        },
        PropSection {
            title: t!("Point Cloud Ex Display").into_owned(),
            props: vec![
                bool_prop(t!("Show Intensity").as_ref(),
                    "ext_pcx_show_intensity",
                    data.show_intensity,
                ),
                bool_prop(t!("Show Cropping").as_ref(), "ext_pcx_show_cropping", data.show_cropping),
                ro_prop(t!("Unknown Flags").as_ref(),
                    "ext_pcx_unknown",
                    format!("{}, {}", data.unknown_bl0, data.unknown_bl1),
                ),
                ro_prop(t!("Stylization Type").as_ref(),
                    "ext_pcx_stylization",
                    data.stylization_type.to_string(),
                ),
                text_prop(t!("Intensity Color Scheme").as_ref(),
                    "ext_pcx_intensity_scheme",
                    &data.intensity_color_scheme,
                ),
                text_prop(t!("Current Color Scheme").as_ref(),
                    "ext_pcx_current_scheme",
                    &data.current_color_scheme,
                ),
                text_prop(t!("Classification Scheme").as_ref(),
                    "ext_pcx_class_scheme",
                    &data.classification_color_scheme,
                ),
                edit_prop(t!("Elevation Min").as_ref(), "ext_pcx_elevation_min", data.elevation_min),
                edit_prop(t!("Elevation Max").as_ref(), "ext_pcx_elevation_max", data.elevation_max),
                ro_prop(t!("Intensity Range").as_ref(),
                    "ext_pcx_intensity_range",
                    format!("{}..{}", data.intensity_min, data.intensity_max),
                ),
                ro_prop(t!("Out Of Range Behavior").as_ref(),
                    "ext_pcx_out_of_range",
                    format!(
                        "intensity {}; elevation {}",
                        data.intensity_out_of_range_behavior,
                        data.elevation_out_of_range_behavior
                    ),
                ),
                bool_prop(t!("Fixed Elevation Range").as_ref(),
                    "ext_pcx_fixed_range",
                    data.elevation_apply_to_fixed_range,
                ),
                bool_prop(t!("Intensity Gradient").as_ref(),
                    "ext_pcx_intensity_gradient",
                    data.intensity_as_gradient,
                ),
                bool_prop(t!("Elevation Gradient").as_ref(),
                    "ext_pcx_elevation_gradient",
                    data.elevation_as_gradient,
                ),
                ro_prop(t!("Croppings").as_ref(), "ext_pcx_croppings", crops),
            ],
        },
    ]
}

fn semantic_properties(
    properties: &[acadrust::objects::SemanticProperty],
) -> String {
    properties
        .iter()
        .map(|property| {
            format!(
                "{} [{}]: {:?}",
                property.subclass, property.code, property.value
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reference_properties(
    references: &[acadrust::objects::ProxyObjectReference],
) -> String {
    references
        .iter()
        .map(|reference| {
            format!(
                "{:X}: {:?}",
                reference.handle.value(),
                reference.kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn properties(entity: &ExtendedEntity) -> Vec<PropSection> {
    match &entity.data {
        ExtendedEntityData::Camera { view_handle } => vec![PropSection {
            title: t!("Camera").into_owned(),
            props: vec![ro_prop(t!("View").as_ref(), "ext_camera_view", handle_text(*view_handle))],
        }],
        ExtendedEntityData::SectionObject(data) => section_properties(data),
        ExtendedEntityData::ArcAlignedText(data) => arc_text_properties(data),
        ExtendedEntityData::RemoteText(data) => remote_text_properties(data),
        ExtendedEntityData::GeoPositionMarker(data) => geo_marker_properties(data),
        ExtendedEntityData::CoordinationModel(data) => vec![PropSection {
            title: t!("Coordination Model").into_owned(),
            props: vec![
                ro_prop(t!("Flags").as_ref(), "ext_coord_flags", data.flags.to_string()),
                ro_prop(t!("Definition").as_ref(),
                    "ext_coord_definition",
                    handle_text(data.definition_handle),
                ),
                edit_prop(t!("Unit Factor").as_ref(), "ext_coord_unit", data.unit_factor),
                ro_prop(t!("Transform").as_ref(),
                    "ext_coord_transform",
                    data.transform
                        .chunks_exact(4)
                        .map(|row| {
                            format!(
                                "{:.6}, {:.6}, {:.6}, {:.6}",
                                row[0], row[1], row[2], row[3]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ],
        }],
        ExtendedEntityData::PointCloud(data) => point_cloud_properties(data),
        ExtendedEntityData::PointCloudEx(data) => point_cloud_ex_properties(data),
        ExtendedEntityData::Proxy(data) => vec![PropSection {
            title: t!("Proxy Entity").into_owned(),
            props: vec![
                ro_prop(t!("Proxy ID").as_ref(), "ext_proxy_id", data.proxy_id.to_string()),
                ro_prop(t!("Class ID").as_ref(), "ext_proxy_class", data.class_id.to_string()),
                ro_prop(t!("DXF Subclass").as_ref(), "ext_proxy_subclass", data.dxf_subclass.clone()),
                ro_prop(t!("Version").as_ref(), "ext_proxy_version", data.version.to_string()),
                ro_prop(t!("DWG Version").as_ref(),
                    "ext_proxy_dwg_version",
                    format!("{}.{}", data.dwg_version, data.maintenance_version),
                ),
                ro_prop(t!("From DXF").as_ref(), "ext_proxy_from_dxf", data.from_dxf.to_string()),
                ro_prop(t!("Graphics").as_ref(),
                    "ext_proxy_graphics",
                    format!("{} bits", data.graphics.bit_count),
                ),
                ro_prop(t!("Payload").as_ref(),
                    "ext_proxy_payload",
                    format!("{} bits", data.payload.bit_count),
                ),
                ro_prop(t!("Text Payload").as_ref(),
                    "ext_proxy_text_payload",
                    format!("{} bits", data.text_payload.bit_count),
                ),
                ro_prop(t!("Object References").as_ref(),
                    "ext_proxy_references",
                    reference_properties(&data.object_ids),
                ),
            ],
        }],
        ExtendedEntityData::OleFrame(data) => vec![PropSection {
            title: t!("OLE Frame").into_owned(),
            props: vec![
                ro_prop(t!("Flag").as_ref(), "ext_ole_flag", data.flag.to_string()),
                ro_prop(t!("Mode").as_ref(), "ext_ole_mode", data.mode.to_string()),
                ro_prop(t!("Storage Size").as_ref(),
                    "ext_ole_size",
                    format!("{} bytes", data.storage.encoded_len()),
                ),
            ],
        }],
        ExtendedEntityData::LayoutPrintConfig(data) => vec![PropSection {
            title: t!("Layout Print Configuration").into_owned(),
            props: vec![
                ro_prop(t!("Class Version").as_ref(),
                    "ext_print_version",
                    data.class_version.to_string(),
                ),
                ro_prop(t!("Flag").as_ref(), "ext_print_flag", data.flag.to_string()),
            ],
        }],
        ExtendedEntityData::Format(data) => vec![PropSection {
            title: t!("Format").into_owned(),
            props: vec![
                ro_prop(t!("DWG Payload").as_ref(),
                    "ext_format_dwg",
                    format!(
                        "{} bytes / {} handle bits",
                        data.raw_dwg_data.as_ref().map_or(0, Vec::len),
                        data.raw_dwg_handle_bits
                    ),
                ),
                ro_prop(t!("DWG Version").as_ref(),
                    "ext_format_version",
                    format!("{:?}", data.raw_dwg_version),
                ),
                ro_prop(t!("DXF Codes").as_ref(),
                    "ext_format_dxf",
                    data.raw_dxf_codes.as_ref().map_or(0, Vec::len).to_string(),
                ),
            ],
        }],
        ExtendedEntityData::Legacy(data) => vec![PropSection {
            title: t!("Legacy Entity").into_owned(),
            props: vec![ro_prop(t!("Data").as_ref(), "ext_legacy_data", format!("{data:#?}"))],
        }],
        ExtendedEntityData::DynamicBlock(data) => vec![PropSection {
            title: t!("Dynamic Block Entity").into_owned(),
            props: vec![
                ro_prop(t!("Class").as_ref(),
                    "ext_dynamic_class",
                    data.entity_dxf_name().unwrap_or("Helper"),
                ),
                ro_prop(t!("Decoded Data").as_ref(), "ext_dynamic_data", format!("{data:#?}")),
            ],
        }],
        ExtendedEntityData::RegisteredClass(data) => vec![PropSection {
            title: t!("Registered Class Entity").into_owned(),
            props: vec![
                ro_prop(t!("DXF Name").as_ref(), "ext_registered_dxf", data.dxf_name.clone()),
                ro_prop(t!("C++ Class").as_ref(),
                    "ext_registered_cpp",
                    data.cpp_class_name.clone(),
                ),
                ro_prop(t!("Properties").as_ref(),
                    "ext_registered_properties",
                    semantic_properties(&data.properties),
                ),
                ro_prop(t!("Payload").as_ref(),
                    "ext_registered_payload",
                    format!("{} bits", data.payload.bit_count),
                ),
                ro_prop(t!("Object References").as_ref(),
                    "ext_registered_references",
                    reference_properties(&data.object_ids),
                ),
            ],
        }],
    }
}

fn set_f64(value: &str, target: &mut f64) {
    if let Some(value) = parse_f64(value) {
        *target = value;
    }
}

fn apply_geom_prop(entity: &mut ExtendedEntity, field: &str, value: &str) {
    match &mut entity.data {
        ExtendedEntityData::SectionObject(data) => match field {
            "ext_section_name" => data.name = value.to_string(),
            "ext_section_top" => set_f64(value, &mut data.top_height),
            "ext_section_bottom" => set_f64(value, &mut data.bottom_height),
            _ => {}
        },
        ExtendedEntityData::ArcAlignedText(data) => match field {
            "ext_arc_text" => data.text = value.to_string(),
            "ext_arc_font" => data.font_name = value.to_string(),
            "ext_arc_big_font" => data.big_font_name = value.to_string(),
            "ext_arc_style" => data.style_name = value.to_string(),
            "ext_arc_radius" => set_f64(value, &mut data.radius),
            "ext_arc_xscale" => set_f64(value, &mut data.x_scale),
            "ext_arc_size" => set_f64(value, &mut data.text_size),
            "ext_arc_spacing" => set_f64(value, &mut data.character_spacing),
            "ext_arc_offset" => set_f64(value, &mut data.offset_from_arc),
            "ext_arc_right" => set_f64(value, &mut data.right_offset),
            "ext_arc_left" => set_f64(value, &mut data.left_offset),
            "ext_arc_start" => {
                if let Some(value) = parse_f64(value) {
                    data.start_angle = value.to_radians();
                }
            }
            "ext_arc_end" => {
                if let Some(value) = parse_f64(value) {
                    data.end_angle = value.to_radians();
                }
            }
            "ext_arc_reverse" => data.reverse = !data.reverse,
            "ext_arc_bold" => data.bold = !data.bold,
            "ext_arc_italic" => data.italic = !data.italic,
            "ext_arc_underlined" => data.underlined = !data.underlined,
            "ext_arc_shx" => data.is_shx = !data.is_shx,
            "ext_arc_wizard" => data.wizard_flag = !data.wizard_flag,
            _ => {}
        },
        ExtendedEntityData::RemoteText(data) => match field {
            "ext_rtext_text" => data.text = value.to_string(),
            "ext_rtext_style" => data.style_name = value.to_string(),
            "ext_rtext_height" => set_f64(value, &mut data.height),
            "ext_rtext_rotation" => {
                if let Some(value) = parse_f64(value) {
                    data.rotation = value.to_radians();
                }
            }
            _ => {}
        },
        ExtendedEntityData::GeoPositionMarker(data) => match field {
            "ext_geo_radius" => set_f64(value, &mut data.radius),
            "ext_geo_notes" => data.notes = value.to_string(),
            "ext_geo_gap" => set_f64(value, &mut data.landing_gap),
            "ext_geo_visible" => data.mtext_visible = !data.mtext_visible,
            "ext_geo_frame" => data.enable_frame_text = !data.enable_frame_text,
            _ => {}
        },
        ExtendedEntityData::CoordinationModel(data) => {
            if field == "ext_coord_unit" {
                set_f64(value, &mut data.unit_factor);
            }
        }
        ExtendedEntityData::PointCloud(data) => match field {
            "ext_pc_file" => data.saved_filename = value.to_string(),
            "ext_pc_ucs" => data.ucs_name = value.to_string(),
            "ext_pc_show_intensity" => data.show_intensity = !data.show_intensity,
            "ext_pc_show_clipping" => data.show_clipping = !data.show_clipping,
            "ext_pc_intensity_min" => set_f64(value, &mut data.minimum_intensity),
            "ext_pc_intensity_max" => set_f64(value, &mut data.maximum_intensity),
            "ext_pc_low_threshold" => set_f64(value, &mut data.low_intensity_threshold),
            "ext_pc_high_threshold" => set_f64(value, &mut data.high_intensity_threshold),
            _ => {}
        },
        ExtendedEntityData::PointCloudEx(data) => match field {
            "ext_pcx_name" => data.name = value.to_string(),
            "ext_pcx_locked" => data.locked = !data.locked,
            "ext_pcx_show_intensity" => data.show_intensity = !data.show_intensity,
            "ext_pcx_show_cropping" => data.show_cropping = !data.show_cropping,
            "ext_pcx_intensity_scheme" => data.intensity_color_scheme = value.to_string(),
            "ext_pcx_current_scheme" => data.current_color_scheme = value.to_string(),
            "ext_pcx_class_scheme" => data.classification_color_scheme = value.to_string(),
            "ext_pcx_elevation_min" => set_f64(value, &mut data.elevation_min),
            "ext_pcx_elevation_max" => set_f64(value, &mut data.elevation_max),
            "ext_pcx_fixed_range" => {
                data.elevation_apply_to_fixed_range = !data.elevation_apply_to_fixed_range
            }
            "ext_pcx_intensity_gradient" => {
                data.intensity_as_gradient = !data.intensity_as_gradient
            }
            "ext_pcx_elevation_gradient" => {
                data.elevation_as_gradient = !data.elevation_as_gradient
            }
            _ => {}
        },
        _ => {}
    }
}

fn apply_point(point: &mut Vector3, apply: GripApply) {
    match apply {
        GripApply::Translate(delta) => {
            point.x += delta.x;
            point.y += delta.y;
            point.z += delta.z;
        }
        GripApply::Absolute(position) => {
            *point = Vector3::new(position.x, position.y, position.z);
        }
    }
}

fn move_extents(
    min: &mut Vector3,
    max: &mut Vector3,
    grip_id: usize,
    apply: GripApply,
) {
    match grip_id {
        0 => apply_point(min, apply),
        1 => apply_point(max, apply),
        2 => {
            let center = (*min + *max) * 0.5;
            let delta = match apply {
                GripApply::Translate(delta) => delta,
                GripApply::Absolute(position) => {
                    position - glam::DVec3::new(center.x, center.y, center.z)
                }
            };
            let offset = Vector3::new(delta.x, delta.y, delta.z);
            *min = *min + offset;
            *max = *max + offset;
        }
        _ => {}
    }
}

fn grips(entity: &ExtendedEntity) -> Vec<GripDef> {
    match &entity.data {
        ExtendedEntityData::SectionObject(data) => data
            .vertices
            .iter()
            .chain(data.back_line_vertices.iter())
            .enumerate()
            .map(|(index, point)| {
                square_grip(index, glam::DVec3::new(point.x, point.y, point.z))
            })
            .collect(),
        ExtendedEntityData::ArcAlignedText(data) => vec![center_grip(
            0,
            glam::DVec3::new(data.center.x, data.center.y, data.center.z),
        )],
        ExtendedEntityData::RemoteText(data) => vec![square_grip(
            0,
            glam::DVec3::new(data.position.x, data.position.y, data.position.z),
        )],
        ExtendedEntityData::GeoPositionMarker(data) => vec![center_grip(
            0,
            glam::DVec3::new(data.position.x, data.position.y, data.position.z),
        )],
        ExtendedEntityData::PointCloud(data) => vec![
            square_grip(
                0,
                glam::DVec3::new(
                    data.extents_min.x,
                    data.extents_min.y,
                    data.extents_min.z,
                ),
            ),
            square_grip(
                1,
                glam::DVec3::new(
                    data.extents_max.x,
                    data.extents_max.y,
                    data.extents_max.z,
                ),
            ),
            center_grip(
                2,
                glam::DVec3::new(
                    (data.extents_min.x + data.extents_max.x) * 0.5,
                    (data.extents_min.y + data.extents_max.y) * 0.5,
                    (data.extents_min.z + data.extents_max.z) * 0.5,
                ),
            ),
        ],
        ExtendedEntityData::PointCloudEx(data) => vec![
            square_grip(
                0,
                glam::DVec3::new(
                    data.extents_min.x,
                    data.extents_min.y,
                    data.extents_min.z,
                ),
            ),
            square_grip(
                1,
                glam::DVec3::new(
                    data.extents_max.x,
                    data.extents_max.y,
                    data.extents_max.z,
                ),
            ),
            center_grip(
                2,
                glam::DVec3::new(
                    (data.extents_min.x + data.extents_max.x) * 0.5,
                    (data.extents_min.y + data.extents_max.y) * 0.5,
                    (data.extents_min.z + data.extents_max.z) * 0.5,
                ),
            ),
        ],
        _ => Vec::new(),
    }
}

fn apply_grip(entity: &mut ExtendedEntity, grip_id: usize, apply: GripApply) {
    match &mut entity.data {
        ExtendedEntityData::SectionObject(data) => {
            if grip_id < data.vertices.len() {
                apply_point(&mut data.vertices[grip_id], apply);
            } else if let Some(point) = data
                .back_line_vertices
                .get_mut(grip_id - data.vertices.len())
            {
                apply_point(point, apply);
            }
        }
        ExtendedEntityData::ArcAlignedText(data) if grip_id == 0 => {
            apply_point(&mut data.center, apply)
        }
        ExtendedEntityData::RemoteText(data) if grip_id == 0 => {
            apply_point(&mut data.position, apply)
        }
        ExtendedEntityData::GeoPositionMarker(data) if grip_id == 0 => {
            let before = data.position;
            apply_point(&mut data.position, apply);
            if let Some(text) = data.embedded_mtext.as_mut() {
                text.insertion_point =
                    text.insertion_point + (data.position - before);
            }
        }
        ExtendedEntityData::PointCloud(data) => {
            let before = (data.extents_min + data.extents_max) * 0.5;
            move_extents(
                &mut data.extents_min,
                &mut data.extents_max,
                grip_id,
                apply,
            );
            if grip_id == 2 {
                let after = (data.extents_min + data.extents_max) * 0.5;
                let delta = after - before;
                data.origin = data.origin + delta;
                data.ucs_origin = data.ucs_origin + delta;
            }
        }
        ExtendedEntityData::PointCloudEx(data) => {
            let before = (data.extents_min + data.extents_max) * 0.5;
            move_extents(
                &mut data.extents_min,
                &mut data.extents_max,
                grip_id,
                apply,
            );
            if grip_id == 2 {
                let after = (data.extents_min + data.extents_max) * 0.5;
                let delta = after - before;
                data.ucs_origin = data.ucs_origin + delta;
                for crop in &mut data.croppings {
                    for point in &mut crop.points {
                        *point = *point + delta;
                    }
                }
            }
        }
        _ => {}
    }
}

fn entity_transform(transform: &EntityTransform) -> Transform {
    match transform {
        EntityTransform::Translate(delta) => {
            Transform::from_translation(Vector3::new(delta.x, delta.y, delta.z))
        }
        EntityTransform::Rotate { center, axis, angle_rad } => {
            Transform::from_translation(Vector3::new(-center.x, -center.y, -center.z))
                .then(&Transform::from_rotation(
                    Vector3::new(axis.x, axis.y, axis.z),
                    *angle_rad,
                ))
                .then(&Transform::from_translation(Vector3::new(
                    center.x, center.y, center.z,
                )))
        }
        EntityTransform::Scale { center, factor } => Transform::from_scaling_with_origin(
            Vector3::new(*factor, *factor, *factor),
            Vector3::new(center.x, center.y, center.z),
        ),
        EntityTransform::Mirror { p1, p2, working_normal } => {
            crate::scene::view::transform::reflection_about_working_line(
                *p1,
                *p2,
                *working_normal,
            )
        }
        EntityTransform::Affine(transform) => *transform,
    }
}

fn scalar_scale(transform: &EntityTransform) -> f64 {
    match transform {
        EntityTransform::Scale { factor, .. } => factor.abs(),
        _ => 1.0,
    }
}

fn transform_extents(min: Vector3, max: Vector3, transform: &Transform) -> (Vector3, Vector3) {
    let corners = [
        Vector3::new(min.x, min.y, min.z),
        Vector3::new(max.x, min.y, min.z),
        Vector3::new(max.x, max.y, min.z),
        Vector3::new(min.x, max.y, min.z),
        Vector3::new(min.x, min.y, max.z),
        Vector3::new(max.x, min.y, max.z),
        Vector3::new(max.x, max.y, max.z),
        Vector3::new(min.x, max.y, max.z),
    ];
    let mut out_min = Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut out_max = Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in corners.map(|point| transform.apply(point)) {
        out_min.x = out_min.x.min(point.x);
        out_min.y = out_min.y.min(point.y);
        out_min.z = out_min.z.min(point.z);
        out_max.x = out_max.x.max(point.x);
        out_max.y = out_max.y.max(point.y);
        out_max.z = out_max.z.max(point.z);
    }
    (out_min, out_max)
}

fn transformed_planar_angle(
    normal: Vector3,
    angle: f64,
    transform: &Transform,
) -> (Vector3, f64) {
    let (axis_x, axis_y, _) = plane_axes(normal);
    let direction = axis_x * angle.cos() + axis_y * angle.sin();
    let new_normal = normalized(transform.apply_rotation(normal), Vector3::UNIT_Z);
    let new_direction = normalized(transform.apply_rotation(direction), Vector3::UNIT_X);
    let (new_x, new_y, _) = plane_axes(new_normal);
    (
        new_normal,
        new_direction.dot(&new_y).atan2(new_direction.dot(&new_x)),
    )
}

fn apply_transform(entity: &mut ExtendedEntity, requested: &EntityTransform) {
    let transform = entity_transform(requested);
    let scale = scalar_scale(requested);
    match &mut entity.data {
        ExtendedEntityData::SectionObject(data) => {
            for point in data
                .vertices
                .iter_mut()
                .chain(data.back_line_vertices.iter_mut())
            {
                *point = transform.apply(*point);
            }
            data.vertical_direction =
                normalized(transform.apply_rotation(data.vertical_direction), Vector3::UNIT_Z);
            data.top_height *= scale;
            data.bottom_height *= scale;
        }
        ExtendedEntityData::ArcAlignedText(data) => {
            let old_normal = data.normal;
            let (_, start) =
                transformed_planar_angle(old_normal, data.start_angle, &transform);
            let (normal, end) =
                transformed_planar_angle(old_normal, data.end_angle, &transform);
            data.center = transform.apply(data.center);
            data.normal = normal;
            data.start_angle = start;
            data.end_angle = end;
            data.radius *= scale;
            data.text_size *= scale;
            data.offset_from_arc *= scale;
            data.right_offset *= scale;
            data.left_offset *= scale;
        }
        ExtendedEntityData::RemoteText(data) => {
            let (normal, rotation) =
                transformed_planar_angle(data.normal, data.rotation, &transform);
            data.position = transform.apply(data.position);
            data.normal = normal;
            data.rotation = rotation;
            data.height *= scale;
        }
        ExtendedEntityData::GeoPositionMarker(data) => {
            data.position = transform.apply(data.position);
            data.radius *= scale;
            data.landing_gap *= scale;
            if let Some(text) = data.embedded_mtext.as_mut() {
                acadrust::Entity::apply_transform(text, &transform);
            }
        }
        ExtendedEntityData::CoordinationModel(data) => {
            let mut matrix = acadrust::types::Matrix4::zero();
            for row in 0..4 {
                for column in 0..4 {
                    matrix.m[row][column] = data.transform[row * 4 + column];
                }
            }
            let composed = transform.matrix * matrix;
            for row in 0..4 {
                for column in 0..4 {
                    data.transform[row * 4 + column] = composed.m[row][column];
                }
            }
            data.unit_factor *= scale;
        }
        ExtendedEntityData::PointCloud(data) => {
            data.origin = transform.apply(data.origin);
            data.ucs_origin = transform.apply(data.ucs_origin);
            data.ucs_x_direction = normalized(
                transform.apply_rotation(data.ucs_x_direction),
                Vector3::UNIT_X,
            );
            data.ucs_y_direction = normalized(
                transform.apply_rotation(data.ucs_y_direction),
                Vector3::UNIT_Y,
            );
            data.ucs_z_direction = normalized(
                transform.apply_rotation(data.ucs_z_direction),
                Vector3::UNIT_Z,
            );
            (data.extents_min, data.extents_max) =
                transform_extents(data.extents_min, data.extents_max, &transform);
            for clip in &mut data.clippings {
                let mut min = Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
                let mut max =
                    Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for point in &clip.vertices {
                    for z in [clip.z_min, clip.z_max] {
                        let point = transform.apply(Vector3::new(point.x, point.y, z));
                        min.x = min.x.min(point.x);
                        min.y = min.y.min(point.y);
                        min.z = min.z.min(point.z);
                        max.x = max.x.max(point.x);
                        max.y = max.y.max(point.y);
                        max.z = max.z.max(point.z);
                    }
                }
                for point in &mut clip.vertices {
                    let transformed = transform.apply(Vector3::new(point.x, point.y, clip.z_min));
                    point.x = transformed.x;
                    point.y = transformed.y;
                }
                if min.z.is_finite() {
                    clip.z_min = min.z;
                    clip.z_max = max.z;
                }
            }
        }
        ExtendedEntityData::PointCloudEx(data) => {
            data.ucs_origin = transform.apply(data.ucs_origin);
            data.ucs_x_direction = normalized(
                transform.apply_rotation(data.ucs_x_direction),
                Vector3::UNIT_X,
            );
            data.ucs_y_direction = normalized(
                transform.apply_rotation(data.ucs_y_direction),
                Vector3::UNIT_Y,
            );
            data.ucs_z_direction = normalized(
                transform.apply_rotation(data.ucs_z_direction),
                Vector3::UNIT_Z,
            );
            (data.extents_min, data.extents_max) =
                transform_extents(data.extents_min, data.extents_max, &transform);
            for crop in &mut data.croppings {
                crop.plane =
                    normalized(transform.apply_rotation(crop.plane), Vector3::UNIT_Z);
                crop.x_direction = normalized(
                    transform.apply_rotation(crop.x_direction),
                    Vector3::UNIT_X,
                );
                crop.y_direction = normalized(
                    transform.apply_rotation(crop.y_direction),
                    Vector3::UNIT_Y,
                );
                for point in &mut crop.points {
                    *point = transform.apply(*point);
                }
            }
        }
        _ => {}
    }
}

impl RenderConvertible for ExtendedEntity {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        to_render(self, document)
    }
}

impl Grippable for ExtendedEntity {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for ExtendedEntity {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for ExtendedEntity {
    fn apply_transform(&mut self, transform: &EntityTransform) {
        apply_transform(self, transform);
    }
}
