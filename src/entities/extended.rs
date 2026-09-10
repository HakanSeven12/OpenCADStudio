use acadrust::entities::{
    ArcAlignedTextData, ExtendedEntity, ExtendedEntityData, GeoPositionMarkerData,
    PointCloudData, PointCloudExData, RemoteTextData, SectionObjectData,
};
use acadrust::types::{Handle, Transform, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_angle_prop, edit_prop, parse_f64, ro_prop, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{
    GripApply, GripDef, GripMenuAction, GripMenuItem, PropSection, PropValue, Property,
};
use crate::scene::model::wire_model::SnapHint;

const NAN: [f64; 3] = [f64::NAN; 3];
const SECTION_SLICE_APP: &str = "IsSlice";
const SECTION_THICKNESS_APP: &str = "ThicknessDepth";
const SECTION_GRIP_LEFT: usize = 100_000;
const SECTION_GRIP_RIGHT: usize = 100_001;
const SECTION_GRIP_TOP: usize = 100_002;
const SECTION_GRIP_BOTTOM: usize = 100_003;
const SECTION_GRIP_NORMAL: usize = 100_004;
const SECTION_GRIP_STATE: usize = 100_005;

pub(crate) fn section_is_slice(entity: &ExtendedEntity) -> bool {
    entity
        .common
        .extended_data
        .get_record(SECTION_SLICE_APP)
        .is_some_and(|record| {
            record
                .values
                .iter()
                .rev()
                .find_map(|value| match value {
                    XDataValue::Integer16(value) => Some(*value != 0),
                    XDataValue::Integer32(value) => Some(*value != 0),
                    _ => None,
                })
                .unwrap_or(false)
        })
}

pub(crate) fn section_slice_depth(entity: &ExtendedEntity) -> Option<f64> {
    entity
        .common
        .extended_data
        .get_record(SECTION_THICKNESS_APP)?
        .values
        .iter()
        .find_map(|value| match value {
            XDataValue::Real(value) | XDataValue::Distance(value) => Some(*value),
            _ => None,
        })
        .filter(|value| value.is_finite())
        .map(f64::abs)
}

pub(crate) fn set_section_slice_metadata(
    entity: &mut ExtendedEntity,
    enabled: bool,
    depth: f64,
) {
    if !enabled {
        entity.common.extended_data.remove_record(SECTION_SLICE_APP);
        entity
            .common
            .extended_data
            .remove_record(SECTION_THICKNESS_APP);
        return;
    }

    let mut slice = ExtendedDataRecord::new(SECTION_SLICE_APP);
    slice.values = vec![XDataValue::Integer16(0), XDataValue::Integer16(1)];
    entity.common.extended_data.upsert_record(slice);

    let mut thickness = ExtendedDataRecord::new(SECTION_THICKNESS_APP);
    thickness.values = vec![XDataValue::Real(if depth.is_finite() {
        depth.abs()
    } else {
        0.0
    })];
    entity.common.extended_data.upsert_record(thickness);
}

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

fn choice_prop(
    label: &str,
    field: &'static str,
    selected: &str,
    options: &[&str],
) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: selected.to_string(),
            options: options.iter().map(|option| (*option).to_string()).collect(),
        },
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
    let vertical = normalized(data.vertical_direction, Vector3::UNIT_Z);
    let offset = |point: &Vector3, distance: f64| {
        [
            point.x + vertical.x * distance,
            point.y + vertical.y * distance,
            point.z + vertical.z * distance,
        ]
    };
    push_chain(
        &mut points,
        data.vertices.iter().map(|p| [p.x, p.y, p.z]),
    );
    push_chain(
        &mut points,
        data.vertices.iter().map(|point| offset(point, data.top_height)),
    );
    push_chain(
        &mut points,
        data.vertices.iter().map(|point| offset(point, -data.bottom_height)),
    );
    for point in &data.vertices {
        push_segment(
            &mut points,
            offset(point, -data.bottom_height),
            offset(point, data.top_height),
        );
    }
    push_chain(
        &mut points,
        data.back_line_vertices.iter().map(|p| [p.x, p.y, p.z]),
    );
    if !data.back_line_vertices.is_empty() {
        push_chain(
            &mut points,
            data.back_line_vertices
                .iter()
                .map(|point| offset(point, data.top_height)),
        );
        push_chain(
            &mut points,
            data.back_line_vertices
                .iter()
                .map(|point| offset(point, -data.bottom_height)),
        );
        for point in &data.back_line_vertices {
            push_segment(
                &mut points,
                offset(point, -data.bottom_height),
                offset(point, data.top_height),
            );
        }
        if let (Some(front), Some(back)) = (
            data.vertices.first().zip(data.vertices.last()),
            data.back_line_vertices
                .first()
                .zip(data.back_line_vertices.last()),
        ) {
            for (a, b) in [(front.0, back.0), (front.1, back.1)] {
                push_segment(&mut points, offset(a, data.top_height), offset(b, data.top_height));
                push_segment(
                    &mut points,
                    offset(a, -data.bottom_height),
                    offset(b, -data.bottom_height),
                );
            }
        }
    }
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

fn section_properties(entity: &ExtendedEntity, data: &SectionObjectData) -> Vec<PropSection> {
    let vertices = data
        .vertices
        .iter()
        .map(|point| vector_text(*point))
        .collect::<Vec<_>>()
        .join("; ");
    let kind = section_kind(entity, data);
    let viewing = section_viewing_direction(data);
    let offset = section_plane_offset(data);
    let depth = section_slice_depth(entity).unwrap_or_else(|| section_depth(data));
    vec![PropSection {
        title: t!("Section Plane").into_owned(),
        props: vec![
            text_prop(t!("Name").as_ref(), "ext_section_name", &data.name),
            choice_prop(
                t!("State").as_ref(),
                "ext_section_state",
                kind,
                &["Plane", "Slice", "Boundary", "Volume"],
            ),
            text_prop(
                t!("Viewing Direction").as_ref(),
                "ext_section_viewing",
                &vector_text(viewing),
            ),
            text_prop(
                t!("Vertical Direction").as_ref(),
                "ext_section_vertical",
                &vector_text(data.vertical_direction),
            ),
            ro_prop(t!("Normal").as_ref(), "ext_section_normal", vector_text(viewing)),
            bool_prop(
                t!("Live Section Enabled").as_ref(),
                "ext_section_live",
                data.flags & 1 != 0,
            ),
            edit_prop(
                t!("Indicator Transparency").as_ref(),
                "ext_section_alpha",
                f64::from(data.indicator_alpha),
            ),
            Property {
                label: t!("Indicator Fill Color").into_owned(),
                field: "indicator_fill_color",
                value: PropValue::ColorChoice(data.indicator_color),
            },
            edit_prop(t!("Elevation").as_ref(), "ext_section_elevation", offset),
            edit_prop(t!("Top Height").as_ref(), "ext_section_top", data.top_height),
            edit_prop(t!("Bottom Height").as_ref(), "ext_section_bottom", data.bottom_height),
            ro_prop(
                t!("Number of Vertices").as_ref(),
                "ext_section_vertex_count",
                data.vertices.len().to_string(),
            ),
            text_prop(t!("Vertices").as_ref(), "ext_section_vertices", &vertices),
            ro_prop(
                t!("Settings").as_ref(),
                "ext_section_settings",
                handle_text(data.settings_handle),
            ),
            choice_prop(
                t!("State2").as_ref(),
                "ext_section_state2",
                kind,
                &["Plane", "Slice", "Boundary", "Volume"],
            ),
            edit_prop(t!("Slice Depth").as_ref(), "ext_section_depth", depth),
            edit_prop(
                t!("Section Plane Offset").as_ref(),
                "ext_section_offset",
                offset,
            ),
        ],
    }]
}

fn section_tangent(data: &SectionObjectData) -> Vector3 {
    let Some((first, last)) = data.vertices.first().zip(data.vertices.last()) else {
        return Vector3::UNIT_X;
    };
    normalized(*last - *first, Vector3::UNIT_X)
}

fn section_viewing_direction(data: &SectionObjectData) -> Vector3 {
    let tangent = section_tangent(data);
    let vertical = normalized(data.vertical_direction, Vector3::UNIT_Z);
    let base = normalized(vertical.cross(&tangent), Vector3::new(0.0, 0.0, -1.0));
    if data.flags & 4 != 0 { base } else { -base }
}

fn section_depth(data: &SectionObjectData) -> f64 {
    data.vertices
        .first()
        .zip(data.back_line_vertices.first())
        .map_or(0.0, |(front, back)| (*back - *front).length())
}

fn section_kind(entity: &ExtendedEntity, data: &SectionObjectData) -> &'static str {
    if section_is_slice(entity) {
        return "Slice";
    }
    match data.state {
        1 => "Plane",
        4 => "Volume",
        2 => "Boundary",
        _ => "Plane",
    }
}

fn section_plane_offset(data: &SectionObjectData) -> f64 {
    let Some(first) = data.vertices.first() else {
        return 0.0;
    };
    -section_viewing_direction(data).dot(first)
}

fn parse_vector(value: &str) -> Option<Vector3> {
    let values = value
        .split(|character: char| character == ',' || character == ';' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 3).then(|| Vector3::new(values[0], values[1], values[2]))
}

fn parse_vertices(value: &str) -> Option<Vec<Vector3>> {
    let values = value
        .split(|character: char| {
            character == ',' || character == ';' || character == ':' || character.is_whitespace()
        })
        .filter(|part| !part.trim().is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 6 || values.len() % 3 != 0 {
        return None;
    }
    let vertices = values
            .chunks_exact(3)
            .map(|point| Vector3::new(point[0], point[1], point[2]))
            .collect::<Vec<_>>();
    let valid = vertices
        .first()
        .zip(vertices.last())
        .is_some_and(|(first, last)| (*last - *first).length() > 1e-12);
    valid.then_some(vertices)
}

fn set_section_depth(data: &mut SectionObjectData, depth: f64) {
    let depth = depth.max(0.0);
    if depth <= 1e-12 || data.vertices.is_empty() {
        data.back_line_vertices.clear();
        return;
    }
    let viewing = section_viewing_direction(data);
    data.back_line_vertices = data
        .vertices
        .iter()
        .map(|point| *point + viewing * depth)
        .collect();
}

fn set_section_kind(data: &mut SectionObjectData, value: &str) {
    let span = data
        .vertices
        .first()
        .zip(data.vertices.last())
        .map_or(1.0, |(first, last)| (*last - *first).length().max(1e-4));
    match value.trim().to_ascii_uppercase().as_str() {
        "SLICE" => {
            data.state = 1;
            data.back_line_vertices.clear();
        }
        "BOUNDARY" => {
            data.state = 2;
            set_section_depth(data, span);
        }
        "VOLUME" => {
            data.state = 4;
            set_section_depth(data, span);
        }
        _ => {
            data.state = 1;
            data.back_line_vertices.clear();
        }
    }
}

fn move_section_to_offset(data: &mut SectionObjectData, desired: f64) {
    let current = section_plane_offset(data);
    let delta = section_viewing_direction(data) * (current - desired);
    for point in data.vertices.iter_mut().chain(data.back_line_vertices.iter_mut()) {
        *point = *point + delta;
    }
}

fn set_viewing_direction(data: &mut SectionObjectData, value: Vector3) {
    if value.length() <= 1e-12 || data.vertices.len() < 2 {
        return;
    }
    let desired = normalized(value, section_viewing_direction(data));
    let tangent = section_tangent(data);
    let vertical = tangent.cross(&desired);
    if vertical.length() <= 1e-12 {
        return;
    }
    data.vertical_direction = normalized(vertical, data.vertical_direction);
    data.flags |= 4;
    let depth = section_depth(data);
    if depth > 0.0 {
        set_section_depth(data, depth);
    }
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
        ExtendedEntityData::SectionObject(data) => section_properties(entity, data),
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

fn apply_section_prop(entity: &mut ExtendedEntity, field: &str, value: &str) {
    let was_slice = section_is_slice(entity);
    let stored_slice_depth = section_slice_depth(entity).unwrap_or(0.0);
    let ExtendedEntityData::SectionObject(data) = &mut entity.data else {
        return;
    };
    let mut slice_update = None;
    match field {
        "ext_section_name" => data.name = value.to_string(),
        "ext_section_state" | "ext_section_state2" => {
            let kind = value.trim().to_ascii_uppercase();
            let span = data
                .vertices
                .first()
                .zip(data.vertices.last())
                .map_or(1.0, |(first, last)| (*last - *first).length().max(1e-4));
            set_section_kind(data, &kind);
            slice_update = Some((kind == "SLICE", (span / 60.0).max(1e-4)));
        }
        "ext_section_viewing" => {
            if let Some(direction) = parse_vector(value) {
                set_viewing_direction(data, direction);
            }
        }
        "ext_section_vertical" => {
            if let Some(direction) = parse_vector(value) {
                let depth = section_depth(data);
                let direction = normalized(direction, data.vertical_direction);
                if direction.dot(&section_tangent(data)).abs() < 1.0 - 1e-9 {
                    data.vertical_direction = direction;
                    if depth > 0.0 {
                        set_section_depth(data, depth);
                    }
                }
            }
        }
        "ext_section_live" => data.flags ^= 1,
        "ext_section_alpha" => {
            if let Ok(alpha) = value.trim().parse::<i16>() {
                data.indicator_alpha = alpha.clamp(0, 100);
            }
        }
        "ext_section_elevation" | "ext_section_offset" => {
            if let Some(offset) = parse_f64(value) {
                move_section_to_offset(data, offset);
            }
        }
        "ext_section_top" => {
            if let Some(height) = parse_f64(value).filter(|height| *height >= 0.0) {
                data.top_height = height;
            }
        }
        "ext_section_bottom" => {
            if let Some(height) = parse_f64(value).filter(|height| *height >= 0.0) {
                data.bottom_height = height;
            }
        }
        "ext_section_vertices" => {
            if let Some(vertices) = parse_vertices(value) {
                let depth = section_depth(data);
                data.vertices = vertices;
                if depth > 0.0 {
                    set_section_depth(data, depth);
                }
            }
        }
        "ext_section_depth" => {
            if let Some(depth) = parse_f64(value) {
                if was_slice {
                    slice_update = Some((true, depth));
                } else {
                    set_section_depth(data, depth);
                }
            }
        }
        _ => {}
    }

    if let Some((enabled, depth)) = slice_update {
        set_section_slice_metadata(entity, enabled, depth);
    } else if was_slice && field != "ext_section_depth" {
        set_section_slice_metadata(entity, true, stored_slice_depth);
    }
}

fn apply_geom_prop(entity: &mut ExtendedEntity, field: &str, value: &str) {
    if matches!(entity.data, ExtendedEntityData::SectionObject(_)) {
        apply_section_prop(entity, field, value);
        return;
    }
    match &mut entity.data {
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
        ExtendedEntityData::SectionObject(data) => {
            let mut grips: Vec<GripDef> = if section_kind(entity, data) == "Plane"
                && data.vertices.len() == 2
                && data.back_line_vertices.is_empty()
            {
                section_plane_edge_grips(data)
            } else {
                data.vertices
                    .iter()
                    .chain(data.back_line_vertices.iter())
                    .enumerate()
                    .map(|(index, point)| {
                        square_grip(index, glam::DVec3::new(point.x, point.y, point.z))
                    })
                    .collect()
            };
            if let Some(right) = data.vertices.last() {
                let vertical = normalized(data.vertical_direction, Vector3::UNIT_Z);
                let edge_center_offset = (data.top_height - data.bottom_height) * 0.5;
                let anchor = *right + vertical * edge_center_offset;
                grips.push(GripDef {
                    id: SECTION_GRIP_STATE,
                    world: glam::DVec3::new(anchor.x, anchor.y, anchor.z),
                    is_midpoint: false,
                    shape: crate::scene::model::object::GripShape::DropdownAdjacent,
                    dir: None,
                    axis: None,
                });
            }
            if let Some(center) = section_center(data) {
                let normal = section_viewing_direction(data);
                let span = data
                    .vertices
                    .first()
                    .zip(data.vertices.last())
                    .map_or(1.0, |(first, last)| (*last - *first).length());
                let offset = (span * 0.08).max(1.0e-6);
                let normal_world = glam::DVec3::new(normal.x, normal.y, normal.z);
                grips.push(GripDef {
                    id: SECTION_GRIP_NORMAL,
                    world: glam::DVec3::new(center.x, center.y, center.z)
                        + normal_world * offset,
                    is_midpoint: false,
                    shape: crate::scene::model::object::GripShape::Triangle,
                    dir: Some(normal_world),
                    axis: Some(normal_world),
                });
            }
            grips
        }
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
            if matches!(
                grip_id,
                SECTION_GRIP_LEFT
                    | SECTION_GRIP_RIGHT
                    | SECTION_GRIP_TOP
                    | SECTION_GRIP_BOTTOM
            ) {
                apply_section_plane_edge_grip(data, grip_id, apply);
            } else if grip_id == SECTION_GRIP_NORMAL {
                let Some(center) = section_center(data) else {
                    return;
                };
                let normal = section_viewing_direction(data);
                let span = data
                    .vertices
                    .first()
                    .zip(data.vertices.last())
                    .map_or(1.0, |(first, last)| (*last - *first).length());
                let offset = (span * 0.08).max(1.0e-6);
                let current = center + normal * offset;
                let delta = match apply {
                    GripApply::Translate(delta) => Vector3::new(delta.x, delta.y, delta.z),
                    GripApply::Absolute(position) => {
                        Vector3::new(position.x, position.y, position.z) - current
                    }
                };
                let constrained = normal * delta.dot(&normal);
                for point in data
                    .vertices
                    .iter_mut()
                    .chain(data.back_line_vertices.iter_mut())
                {
                    *point = *point + constrained;
                }
            } else if grip_id < data.vertices.len() {
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

fn section_center(data: &SectionObjectData) -> Option<Vector3> {
    let (first, last) = data.vertices.first().zip(data.vertices.last())?;
    Some((*first + *last) * 0.5)
}

fn section_plane_edge_grips(data: &SectionObjectData) -> Vec<GripDef> {
    let Some((first, last)) = data.vertices.first().zip(data.vertices.last()) else {
        return Vec::new();
    };
    let center = (*first + *last) * 0.5;
    let tangent = section_tangent(data);
    let vertical = normalized(data.vertical_direction, Vector3::UNIT_Z);
    let edge_center_offset = (data.top_height - data.bottom_height) * 0.5;
    let definitions = [
        (
            SECTION_GRIP_LEFT,
            *first + vertical * edge_center_offset,
            vertical,
            None,
        ),
        (
            SECTION_GRIP_RIGHT,
            *last + vertical * edge_center_offset,
            vertical,
            None,
        ),
        (
            SECTION_GRIP_TOP,
            center + vertical * data.top_height,
            tangent,
            Some(vertical),
        ),
        (
            SECTION_GRIP_BOTTOM,
            center - vertical * data.bottom_height,
            tangent,
            Some(vertical),
        ),
    ];
    definitions
        .into_iter()
        .map(|(id, world, edge, axis)| GripDef {
            id,
            world: glam::DVec3::new(world.x, world.y, world.z),
            is_midpoint: false,
            shape: crate::scene::model::object::GripShape::Rectangle,
            dir: Some(glam::DVec3::new(edge.x, edge.y, edge.z)),
            axis: axis.map(|axis| glam::DVec3::new(axis.x, axis.y, axis.z)),
        })
        .collect()
}

fn apply_section_plane_edge_grip(
    data: &mut SectionObjectData,
    grip_id: usize,
    apply: GripApply,
) {
    let Some((first, last)) = data.vertices.first().zip(data.vertices.last()) else {
        return;
    };
    let center = (*first + *last) * 0.5;
    let tangent = section_tangent(data);
    let vertical = normalized(data.vertical_direction, Vector3::UNIT_Z);
    let edge_center_offset = (data.top_height - data.bottom_height) * 0.5;
    let current = match grip_id {
        SECTION_GRIP_LEFT => *first + vertical * edge_center_offset,
        SECTION_GRIP_RIGHT => *last + vertical * edge_center_offset,
        SECTION_GRIP_TOP => center + vertical * data.top_height,
        SECTION_GRIP_BOTTOM => center - vertical * data.bottom_height,
        _ => return,
    };
    let delta = match apply {
        GripApply::Translate(delta) => Vector3::new(delta.x, delta.y, delta.z),
        GripApply::Absolute(position) => {
            Vector3::new(position.x, position.y, position.z) - current
        }
    };
    let span = (*last - *first).length();
    let minimum_span = 1.0e-6;
    let in_plane_vertical = normalized(
        vertical - tangent * vertical.dot(&tangent),
        vertical,
    );
    let vertical_change = in_plane_vertical * delta.dot(&in_plane_vertical);
    match grip_id {
        SECTION_GRIP_LEFT => {
            let tangent_change = delta
                .dot(&tangent)
                .min((span - minimum_span).max(0.0));
            data.vertices[0] = data.vertices[0] + tangent * tangent_change + vertical_change;
        }
        SECTION_GRIP_RIGHT => {
            let last = data.vertices.len() - 1;
            let tangent_change = delta
                .dot(&tangent)
                .max((-span + minimum_span).min(0.0));
            data.vertices[last] =
                data.vertices[last] + tangent * tangent_change + vertical_change;
        }
        SECTION_GRIP_TOP => {
            data.top_height = (data.top_height + delta.dot(&vertical)).max(0.0);
        }
        SECTION_GRIP_BOTTOM => {
            data.bottom_height = (data.bottom_height - delta.dot(&vertical)).max(0.0);
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
    let slice_depth = section_is_slice(entity)
        .then(|| section_slice_depth(entity).unwrap_or(0.0) * scale);
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
    if let Some(depth) = slice_depth {
        set_section_slice_metadata(entity, true, depth);
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

    fn grip_menu(&self, grip_id: usize) -> Vec<GripMenuItem> {
        if grip_id != SECTION_GRIP_STATE {
            return Vec::new();
        }
        let ExtendedEntityData::SectionObject(data) = &self.data else {
            return Vec::new();
        };
        let current = section_kind(self, data);
        vec![
            GripMenuItem {
                label: if current == "Plane" { "✓ Plane" } else { "Plane" },
                action: GripMenuAction::SectionPlane,
            },
            GripMenuItem {
                label: if current == "Slice" { "✓ Slice" } else { "Slice" },
                action: GripMenuAction::SectionSlice,
            },
            GripMenuItem {
                label: if current == "Boundary" {
                    "✓ Boundary"
                } else {
                    "Boundary"
                },
                action: GripMenuAction::SectionBoundary,
            },
            GripMenuItem {
                label: if current == "Volume" { "✓ Volume" } else { "Volume" },
                action: GripMenuAction::SectionVolume,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: GripMenuAction) {
        if grip_id != SECTION_GRIP_STATE
            || !matches!(self.data, ExtendedEntityData::SectionObject(_))
        {
            return;
        }
        let value = match action {
            GripMenuAction::SectionPlane => "Plane",
            GripMenuAction::SectionSlice => "Slice",
            GripMenuAction::SectionBoundary => "Boundary",
            GripMenuAction::SectionVolume => "Volume",
            _ => return,
        };
        apply_section_prop(self, "ext_section_state", value);
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
