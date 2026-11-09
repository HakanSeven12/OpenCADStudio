//! Civil 3D entities, drawn from the geometry their class body stores.
//!
//! The drawing's proxy graphics for these classes are only a bounding box,
//! so each kind is rebuilt from the decoded record.

use document::classes::arch::ArchEntityData;
use document::classes::arch::ArchObject;
use document::classes::civil::{parse_format, AssemblyLayout, property, CivilKind, LabelPart, LabelText, ProfileViewFrame, TinSurface};
use document::objects::ObjectType;
use document::entities::{EntityType, ExtendedEntityData};
use document::types::Handle;

use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::convert::proxy_graphics::{Decoded, ProxyPolyline, ProxyStyle, ProxyText};
use document::types::Vector2;

const NAN: [f64; 3] = [f64::NAN; 3];

type Key = (usize, usize);

/// Decoded points of the slow kinds (TIN, corridor), keyed by the record
/// bytes they came from; an edited record has new bytes and decodes again.
// ponytail: never evicts; bound it if drawings are edited heavily in one session.
fn cached(
    data: &ArchEntityData,
    build: impl FnOnce(&ArchEntityData) -> Option<Vec<[f64; 3]>>,
) -> Vec<[f64; 3]> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<Key, Arc<Vec<[f64; 3]>>>>> = OnceLock::new();
    let Some(raw) = data.raw_dwg_data.as_ref() else {
        return build(data).unwrap_or_default();
    };
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        (hasher.finish() as usize, raw.len())
    };
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit.as_ref().clone();
    }
    let points = build(data).unwrap_or_default();
    if let Ok(mut c) = cache.lock() {
        c.insert(key, Arc::new(points.clone()));
    }
    points
}

pub(crate) fn render(data: &ArchEntityData, handle: u64, document: &document::Document) -> Option<RenderEntity> {
    let points = match data.civil_kind()? {
        CivilKind::TinSurface => cached(data, |data| tin_edges(&data.tin_surface()?).into()),
        CivilKind::Alignment => chains(
            data.alignment_polylines()
                .into_iter()
                .map(|run| run.into_iter().map(|p| [p.x, p.y, 0.0]).collect()),
        ),
        CivilKind::ProfileView => profile_view(data, document)?,
        CivilKind::Assembly => assembly_baseline(data)?,
        CivilKind::Subassembly => subassembly_links(data, handle, document)?,
        CivilKind::SampleLineGroup => sample_line_points(data, document)?,
        CivilKind::Corridor => cached(data, |data| {
            Some(chains(
                data.corridor_feature_lines()
                    .into_iter()
                    .map(|run| run.into_iter().map(|(_, p)| [p.x, p.y, p.z]).collect()),
            ))
        }),
        _ => return None,
    };
    (points.len() >= 2).then(|| RenderEntity {
        object: RenderObject::Lines(points),
        snap_pts: Vec::new(),
        tangent_geoms: Vec::new(),
        key_vertices: Vec::new(),
        fill_tris: Vec::new(),
        pick_tris: Vec::new(),
    })
}

/// Vertical marker through the baseline point, as tall as the assembly.
fn assembly_baseline(data: &ArchEntityData) -> Option<Vec<[f64; 3]>> {
    let origin = data.assembly_layout()?.origin;
    let ys = data.profiles.iter().flatten().map(|p| p.y);
    let (low, high) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), y| (l.min(y), h.max(y)));
    (low < high).then(|| vec![[origin.x, low, 0.0], [origin.x, high, 0.0]])
}

/// Links of a subassembly shape, placed at its attachment point.
fn subassembly_links(data: &ArchEntityData, handle: u64, document: &document::Document) -> Option<Vec<[f64; 3]>> {
    let shape = data.subassembly_shape()?;
    let assembly = arch_of(document, data.subassembly_assembly()?, CivilKind::Assembly)?;
    let layout = assembly.assembly_layout()?;
    let origin = subassembly_origin(document, &layout, handle, layout.attachments.len())?;
    Some(chains(shape.links.iter().map(|link| {
        link.iter()
            .map(|&i| {
                let p = origin + shape.points[i as usize];
                [p.x, p.y, 0.0]
            })
            .collect()
    })))
}

/// Drawing position of a subassembly's origin: the parent point it hangs
/// from, down to the assembly baseline.
fn subassembly_origin(
    document: &document::Document,
    layout: &AssemblyLayout,
    handle: u64,
    depth: usize,
) -> Option<Vector2> {
    let attachment = layout.attachments.iter().find(|a| a.subassembly == handle)?;
    if attachment.parent == 0 {
        return Some(Vector2::new(layout.origin.x, layout.origin.y));
    }
    let parent = arch_of(document, attachment.parent, CivilKind::Subassembly)?.subassembly_shape()?;
    let point = *parent.points.get(attachment.point as usize)?;
    Some(subassembly_origin(document, layout, attachment.parent, depth.checked_sub(1)?)? + point)
}

/// Every edge of the drawn triangles once, NaN-separated.
fn tin_edges(surface: &TinSurface) -> Vec<[f64; 3]> {
    let at = |v: u32| {
        let p = surface.vertices[v as usize].position;
        [p.x, p.y, p.z]
    };
    let mut points = Vec::new();
    for (index, triangle) in surface.triangles.iter().enumerate() {
        if !triangle.is_visible() {
            continue;
        }
        for edge in 0..3 {
            let neighbor = triangle.neighbors[edge];
            // A shared edge between two drawn triangles is emitted by the
            // lower-numbered one.
            let shared = neighbor >= 0
                && surface.triangles[neighbor as usize].is_visible()
                && (neighbor as usize) < index;
            if shared {
                continue;
            }
            if !points.is_empty() {
                points.push(NAN);
            }
            points.push(at(triangle.vertices[edge]));
            points.push(at(triangle.vertices[(edge + 1) % 3]));
        }
    }
    points
}

/// NaN-separated polylines.
fn chains(runs: impl Iterator<Item = Vec<[f64; 3]>>) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for run in runs.filter(|run| run.len() > 1) {
        if !points.is_empty() {
            points.push(NAN);
        }
        points.extend(run);
    }
    points
}

/// Frame, grid, and the profiles the view lists.
fn profile_view(data: &ArchEntityData, document: &document::Document) -> Option<Vec<[f64; 3]>> {
    let frame = data.profile_view_frame()?;
    let at = |station: f64, elevation: f64| {
        let p = frame.to_drawing(station, elevation);
        [p.x, p.y, 0.0]
    };
    let mut runs: Vec<Vec<[f64; 3]>> = Vec::new();
    let (s0, s1, e0, e1) = (
        frame.station_min,
        frame.station_max,
        frame.elevation_min,
        frame.elevation_max,
    );
    runs.push(vec![at(s0, e0), at(s1, e0), at(s1, e1), at(s0, e1), at(s0, e0)]);
    for station in grid(s0, s1, 100.0) {
        runs.push(vec![at(station, e0), at(station, e1)]);
    }
    for elevation in grid(e0, e1, 10.0) {
        runs.push(vec![at(s0, elevation), at(s1, elevation)]);
    }
    for handle in data.decode().handles {
        if let Some(profile) = profile_entity(document, handle) {
            for run in profile.profile_polylines() {
                runs.extend(clip_to_view(&frame, &run));
            }
        }
    }
    Some(chains(runs.into_iter()))
}

/// Interior grid values on multiples of `step`.
fn grid(from: f64, to: f64, step: f64) -> impl Iterator<Item = f64> {
    let first = (from / step).floor() as i64 + 1;
    let last = (to / step).ceil() as i64 - 1;
    let count = (last - first + 1).clamp(0, 500);
    (first..first + count)
        .map(move |i| i as f64 * step)
        .filter(move |v| *v > from && *v < to)
}

fn profile_entity(document: &document::Document, handle: u64) -> Option<&ArchEntityData> {
    let EntityType::Extended(entity) = document.get_entity(Handle::new(handle))? else {
        return None;
    };
    let ExtendedEntityData::Arch(data) = &entity.data else {
        return None;
    };
    (data.civil_kind() == Some(CivilKind::Profile)).then_some(data)
}

/// Pieces of a station/elevation run inside the view's station range.
fn clip_to_view(frame: &ProfileViewFrame, run: &[document::types::Vector2]) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    let mut current = Vec::new();
    for p in run {
        if p.x >= frame.station_min && p.x <= frame.station_max {
            let q = frame.to_drawing(p.x, p.y);
            current.push([q.x, q.y, 0.0]);
        } else if current.len() > 1 {
            out.push(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// Civil 3D's text height when a label component does not set its own
/// (2.5 mm plotted).
const DEFAULT_PLOTTED_HEIGHT: f64 = 0.0025;

/// Lines and generated text for Civil 3D objects that carry text: label
/// groups and profile views. `None` for everything else.
pub(crate) fn decoded(data: &ArchEntityData, document: &document::Document) -> Option<Decoded> {
    let mut out = Decoded::default();
    match data.civil_kind()? {
        CivilKind::ProfileView => profile_view_text(data, document, &mut out)?,
        CivilKind::Label => label_group(data, document, &mut out)?,
        _ => return None,
    }
    (!out.is_empty()).then_some(out)
}

fn line(out: &mut Decoded, points: Vec<[f64; 3]>) {
    out.polylines.push(ProxyPolyline {
        points,
        style: ProxyStyle::default(),
    });
}

/// A label style as drawn: its layer and text components.
struct LabelStyle {
    layer: Option<u32>,
    components: Vec<LabelText>,
}

fn object_arch(document: &document::Document, handle: u64) -> Option<&ArchObject> {
    match document.objects.get(&Handle::new(handle))? {
        ObjectType::Arch(object) => Some(object),
        _ => None,
    }
}

/// Collector style among `handles`: its first reference is the label layer,
/// the rest its components.
fn label_style(document: &document::Document, handles: &[u64]) -> Option<LabelStyle> {
    let style = handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_LABEL_COLLECTOR_STYLE")?;
    Some(collector(document, style))
}

fn collector(document: &document::Document, style: &ArchObject) -> LabelStyle {
    let handles = style.decode().handles;
    let layer = handles.iter().find_map(|h| {
        document
            .layers
            .iter()
            .position(|layer| layer.handle.value() == *h)
            .map(|i| i as u32)
    });
    let mut seen = Vec::new();
    let components = handles
        .iter()
        .filter(|h| {
            let fresh = !seen.contains(*h);
            seen.push(**h);
            fresh
        })
        .filter_map(|h| object_arch(document, *h)?.label_text())
        .collect();
    LabelStyle { layer, components }
}

/// Drawing units per plotted unit. Civil 3D stores plotted sizes in the
/// drawing's unit (0.0025 = 2.5 mm in a metre drawing), so the drawing scale
/// is the ratio the current annotation scale is named after; a metre
/// drawing's "1:1000" is often stored converted as 1 = 1.
fn drawing_scale(document: &document::Document) -> f64 {
    let header = &document.header;
    header
        .current_annotation_scale
        .split_once(':')
        .and_then(|(paper, drawing)| {
            let (paper, drawing) = (paper.trim().parse::<f64>().ok()?, drawing.trim().parse::<f64>().ok()?);
            (paper > 0.0 && drawing > 0.0).then(|| drawing / paper)
        })
        .or_else(|| (header.annotation_scale_value > 1e-9).then(|| 1.0 / header.annotation_scale_value))
        .unwrap_or(1.0)
}

/// A component's lines with every field filled from `value`; `None` when a
/// field has no value for this label.
fn component_lines(component: &LabelText, value: &dyn Fn(u32, &[u32]) -> Option<String>) -> Option<Vec<String>> {
    let mut lines = vec![String::new()];
    for part in &component.content {
        match part {
            LabelPart::Text(text) => lines.last_mut().unwrap().push_str(text),
            LabelPart::NewLine => lines.push(String::new()),
            LabelPart::Field { property, options } => {
                lines.last_mut().unwrap().push_str(&value(*property, options)?)
            }
        }
    }
    Some(lines)
}

/// Place a label: each component's lines stacked from `at` along `angle`,
/// offset across it by the component's own offset.
fn place(
    out: &mut Decoded,
    document: &document::Document,
    style: &LabelStyle,
    at: Vector2,
    angle: f64,
    value: &dyn Fn(u32, &[u32]) -> Option<String>,
) {
    let scale = drawing_scale(document);
    let across = Vector2::new(-angle.sin(), angle.cos());
    let mut row = 0.0;
    for component in &style.components {
        let Some(lines) = component_lines(component, value) else {
            continue;
        };
        let height = component.height.unwrap_or(DEFAULT_PLOTTED_HEIGHT) * scale;
        let shift = component.y_offset.unwrap_or(0.0) * scale;
        for line_text in lines {
            if line_text.trim().is_empty() {
                continue;
            }
            let p = at + across * (shift - row);
            out.texts.push(ProxyText {
                position: [p.x, p.y, 0.0],
                height,
                rotation: angle,
                width_factor: 1.0,
                oblique: 0.0,
                text: line_text,
                font: String::new(),
                style: ProxyStyle {
                    layer: style.layer,
                    ..ProxyStyle::default()
                },
            });
            row += height * 1.5;
        }
    }
}

fn arch_of(document: &document::Document, handle: u64, kind: CivilKind) -> Option<&ArchEntityData> {
    let EntityType::Extended(entity) = document.get_entity(Handle::new(handle))? else {
        return None;
    };
    let ExtendedEntityData::Arch(data) = &entity.data else {
        return None;
    };
    (data.civil_kind() == Some(kind)).then_some(data)
}

/// Point and unit direction at `distance` along a polyline.
fn point_at(run: &[Vector2], distance: f64) -> Option<(Vector2, Vector2)> {
    let mut left = distance;
    for pair in run.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(&b);
        if length <= 0.0 {
            continue;
        }
        if left <= length {
            let d = (b - a) * (1.0 / length);
            return Some((a + d * left, d));
        }
        left -= length;
    }
    None
}

/// Reading angle that keeps text upright.
fn upright(angle: f64) -> f64 {
    let a = angle.rem_euclid(std::f64::consts::TAU);
    if a > std::f64::consts::FRAC_PI_2 && a <= 3.0 * std::f64::consts::FRAC_PI_2 {
        a - std::f64::consts::PI
    } else {
        a
    }
}

fn label_group(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) -> Option<()> {
    use property as p;
    let handles = data.decode().handles;
    let name = data.dxf_name.as_str();
    let alignment = handles
        .iter()
        .find_map(|h| arch_of(document, *h, CivilKind::Alignment));
    let style = label_style(document, &handles);
    match name {
        "AECC_ALIGNMENT_STATION_LABEL_GROUP" | "AECC_ALIGNMENT_MINOR_STATION_LABEL_GROUP" => {
            let alignment = alignment?;
            let major = name == "AECC_ALIGNMENT_STATION_LABEL_GROUP";
            let start = alignment.alignment_start_station().unwrap_or(0.0);
            let runs = alignment.alignment_polylines();
            let run = runs.first()?;
            let scale = drawing_scale(document);
            let tick = if major { 0.002 } else { 0.001 } * scale;
            for station in data.label_stations() {
                let Some((at, d)) = point_at(run, station - start) else {
                    continue;
                };
                let n = Vector2::new(-d.y, d.x);
                let (a, b) = (at - n * tick, at + n * tick);
                line(out, vec![[a.x, a.y, 0.0], [b.x, b.y, 0.0]]);
                if let Some(style) = &style {
                    let value = |property: u32, options: &[u32]| {
                        (property == p::STATION).then(|| LabelPart::format_value(options, station))
                    };
                    place(out, document, style, b, upright(n.y.atan2(n.x)), &value);
                }
            }
        }
        "AECC_ALIGNMENT_GEOMPOINT_LABEL_GROUP" => {
            let alignment = alignment?;
            let style = style?;
            let start = alignment.alignment_start_station().unwrap_or(0.0);
            let runs = alignment.alignment_polylines();
            let run = runs.first()?;
            let reach = 0.006 * drawing_scale(document);
            for point in alignment.alignment_geometry_points() {
                let Some((at, d)) = point_at(run, point.distance) else {
                    continue;
                };
                let n = Vector2::new(-d.y, d.x);
                let tip = at + n * reach;
                line(out, vec![[at.x, at.y, 0.0], [tip.x, tip.y, 0.0]]);
                let station = start + point.distance;
                let value = |property: u32, options: &[u32]| match property {
                    p::GEOMETRY_POINT => Some(point.name.to_string()),
                    p::STATION => Some(LabelPart::format_value(options, station)),
                    _ => None,
                };
                place(out, document, &style, tip, upright(n.y.atan2(n.x)), &value);
            }
        }
        "AECC_SAMPLELINE_LABELING" => {
            let group = handles
                .iter()
                .find_map(|h| arch_of(document, *h, CivilKind::SampleLineGroup))?;
            let style = style?;
            let template = group.sample_line_name_template().unwrap_or_default();
            for (station, left, right) in sample_lines(group, document)? {
                let name: String = template
                    .iter()
                    .map(|part| match part {
                        LabelPart::Text(text) => text.clone(),
                        LabelPart::Field { options, .. } => LabelPart::format_value(options, station),
                        LabelPart::NewLine => String::new(),
                    })
                    .collect();
                let value = |property: u32, _: &[u32]| (property == p::SAMPLE_LINE_NAME).then(|| name.clone());
                let d = right - left;
                place(out, document, &style, left, upright(d.y.atan2(d.x)), &value);
            }
        }
        "AECC_STATION_ELEVATION_LABEL" => {
            let style = style?;
            let at = data.label_anchor()?;
            let frame = document
                .entities()
                .filter_map(|e| match e {
                    EntityType::Extended(x) => match &x.data {
                        ExtendedEntityData::Arch(view) => view.profile_view_frame(),
                        _ => None,
                    },
                    _ => None,
                })
                .find(|f| {
                    let top = f.to_drawing(f.station_max, f.elevation_max);
                    at.x >= f.origin.x && at.y >= f.origin.y && at.x <= top.x && at.y <= top.y
                })?;
            let station = frame.station_min + (at.x - frame.origin.x);
            let elevation = frame.elevation_min + (at.y - frame.origin.y) / frame.vertical_scale;
            let value = |property: u32, options: &[u32]| match property {
                p::STATION => Some(LabelPart::format_value(options, station)),
                ELEVATION => Some(LabelPart::format_value(options, elevation)),
                _ => None,
            };
            place(out, document, &style, at, 0.0, &value);
        }
        "AECC_PROFILEDATABAND_LABELGROUP" => {
            let view = handles
                .iter()
                .find_map(|h| arch_of(document, *h, CivilKind::ProfileView))?;
            let frame = view.profile_view_frame()?;
            let view_handles = view.decode().handles;
            let profiles: Vec<_> = view_handles
                .iter()
                .filter_map(|h| arch_of(document, *h, CivilKind::Profile))
                .collect();
            let eg = profiles.first().map(|p| p.profile_polylines()).unwrap_or_default();
            let fg = profiles.get(1).map(|p| p.profile_polylines()).unwrap_or_default();
            // The band's major-station collector: the one labelling stations.
            let band = view_handles
                .iter()
                .filter_map(|h| object_arch(document, *h))
                .find(|o| o.dxf_name == "AECC_PROFILE_VIEW_DATA_BAND_STYLE")?;
            let style = band
                .decode()
                .handles
                .iter()
                .filter_map(|h| object_arch(document, *h))
                .filter(|o| o.dxf_name == "AECC_LABEL_COLLECTOR_STYLE")
                .map(|o| collector(document, o))
                .find(|s| {
                    s.components.iter().any(|c| {
                        c.content
                            .iter()
                            .any(|part| matches!(part, LabelPart::Field { property, .. } if *property == p::STATION))
                    })
                })?;
            let below = 0.004 * drawing_scale(document);
            for station in data.label_stations() {
                let x = frame.to_drawing(station, frame.elevation_min).x;
                let value = |property: u32, options: &[u32]| match property {
                    p::STATION => Some(LabelPart::format_value(options, station)),
                    EG_ELEVATION => elevation_at(&eg, station).map(|v| LabelPart::format_value(options, v)),
                    FG_ELEVATION => elevation_at(&fg, station).map(|v| LabelPart::format_value(options, v)),
                    _ => None,
                };
                place(
                    out,
                    document,
                    &style,
                    Vector2::new(x, frame.origin.y - below),
                    std::f64::consts::FRAC_PI_2,
                    &value,
                );
            }
        }
        "AECC_VALIGNMENT_LINE_LABEL_GROUP"
        | "AECC_VALIGNMENT_PVI_LABEL_GROUP"
        | "AECC_VALIGNMENT_SAGCURVE_LABEL_GROUP"
        | "AECC_VALIGNMENT_CRESTCURVE_LABEL_GROUP" => {
            let style = style?;
            let profile = handles
                .iter()
                .find_map(|h| arch_of(document, *h, CivilKind::Profile))?;
            let view = handles
                .iter()
                .find_map(|h| arch_of(document, *h, CivilKind::ProfileView))?;
            let frame = view.profile_view_frame()?;
            let pvis = profile.profile_pvis();
            let inside = |q: Vector2| q.x >= frame.station_min && q.x <= frame.station_max;
            let at = |q: Vector2| frame.to_drawing(q.x, q.y);
            let scale = drawing_scale(document);
            for (index, pvi) in pvis.iter().enumerate() {
                let grade = |a: &document::classes::civil::Pvi, b: &document::classes::civil::Pvi| {
                    (b.point.y - a.point.y) / (b.point.x - a.point.x)
                };
                let grade_in = index.checked_sub(1).map(|i| grade(&pvis[i], pvi));
                let grade_out = pvis.get(index + 1).map(|n| grade(pvi, n));
                match name {
                    "AECC_VALIGNMENT_LINE_LABEL_GROUP" => {
                        let (Some(next), Some(g)) = (pvis.get(index + 1), grade_out) else {
                            continue;
                        };
                        let mid = (pvi.point + next.point) * 0.5;
                        if !inside(mid) {
                            continue;
                        }
                        let (a, b) = (at(pvi.point), at(next.point));
                        let value = |property: u32, options: &[u32]| {
                            (property == p::TANGENT_GRADE).then(|| LabelPart::format_value(options, g))
                        };
                        place(out, document, &style, at(mid), upright((b.y - a.y).atan2(b.x - a.x)), &value);
                    }
                    "AECC_VALIGNMENT_PVI_LABEL_GROUP" => {
                        if !inside(pvi.point) {
                            continue;
                        }
                        let q = at(pvi.point);
                        let top = q + Vector2::new(0.0, 0.04 * scale);
                        line(out, vec![[q.x, q.y, 0.0], [top.x, top.y, 0.0]]);
                        let value = |property: u32, options: &[u32]| match property {
                            p::PVI_STATION => Some(LabelPart::format_value(options, pvi.point.x)),
                            p::PVI_ELEVATION => Some(LabelPart::format_value(options, pvi.point.y)),
                            _ => None,
                        };
                        place(out, document, &style, top, std::f64::consts::FRAC_PI_2, &value);
                    }
                    _ => {
                        let (Some(g0), Some(g1)) = (grade_in, grade_out) else {
                            continue;
                        };
                        let sag = g1 > g0;
                        let wanted = (name == "AECC_VALIGNMENT_SAGCURVE_LABEL_GROUP") == sag;
                        let length = pvi.curve_length;
                        if length <= 0.0 || !wanted || !inside(pvi.point) {
                            continue;
                        }
                        let bvc = Vector2::new(pvi.point.x - length / 2.0, pvi.point.y - g0 * length / 2.0);
                        let evc = Vector2::new(pvi.point.x + length / 2.0, pvi.point.y + g1 * length / 2.0);
                        // Extreme point of the parabola, clamped to the curve.
                        let t = (-g0 / (g1 - g0)).clamp(0.0, 1.0) * length;
                        let extreme = Vector2::new(bvc.x + t, bvc.y + g0 * t + (g1 - g0) * t * t / (2.0 * length));
                        let k = length / ((g1 - g0).abs() * 100.0);
                        let value = |property: u32, options: &[u32]| {
                            let v = match property {
                                p::PVI_STATION => pvi.point.x,
                                p::PVI_ELEVATION => pvi.point.y,
                                p::BVC_STATION => bvc.x,
                                p::BVC_ELEVATION => bvc.y,
                                p::EVC_STATION => evc.x,
                                p::EVC_ELEVATION => evc.y,
                                p::HIGH_POINT_STATION | p::LOW_POINT_STATION => extreme.x,
                                p::HIGH_POINT_ELEVATION | p::LOW_POINT_ELEVATION => extreme.y,
                                p::K_VALUE => k,
                                p::CURVE_LENGTH => length,
                                _ => return None,
                            };
                            Some(LabelPart::format_value(options, v))
                        };
                        let (a, b) = (at(bvc), at(evc));
                        let dy = if sag { -0.03 } else { 0.03 } * scale;
                        let mark = |out: &mut Decoded, q: Vector2| {
                            line(out, vec![[q.x, q.y, 0.0], [q.x, q.y + dy, 0.0]]);
                        };
                        mark(out, a);
                        mark(out, b);
                        line(out, vec![[a.x, a.y + dy, 0.0], [b.x, b.y + dy, 0.0]]);
                        let top = at(pvi.point) + Vector2::new(0.0, dy);
                        place(out, document, &style, top, std::f64::consts::FRAC_PI_2, &value);
                    }
                }
            }
        }
        _ => return None,
    }
    Some(())
}

/// Station/elevation label properties Civil 3D writes for profile views.
const ELEVATION: u32 = 33_652_743;
const EG_ELEVATION: u32 = 33_661_044;
const FG_ELEVATION: u32 = 33_661_045;

/// Each sample line as a perpendicular across the parent alignment.
fn sample_lines(
    data: &ArchEntityData,
    document: &document::Document,
) -> Option<Vec<(f64, Vector2, Vector2)>> {
    let alignment = data
        .decode()
        .handles
        .iter()
        .find_map(|h| arch_of(document, *h, CivilKind::Alignment))?;
    let lines = data.sample_lines()?;
    let start = alignment.alignment_start_station().unwrap_or(0.0);
    let runs = alignment.alignment_polylines();
    let run = runs.first()?;
    Some(
        lines
            .stations
            .iter()
            .filter_map(|station| {
                let (p, d) = point_at(run, station - start)?;
                let n = Vector2::new(-d.y, d.x);
                Some((*station, p + n * lines.left, p - n * lines.right))
            })
            .collect(),
    )
}

fn sample_line_points(data: &ArchEntityData, document: &document::Document) -> Option<Vec<[f64; 3]>> {
    Some(chains(
        sample_lines(data, document)?
            .into_iter()
            .map(|(_, a, b)| vec![[a.x, a.y, 0.0], [b.x, b.y, 0.0]]),
    ))
}

/// Elevation of a station on station/elevation runs.
fn elevation_at(runs: &[Vec<Vector2>], station: f64) -> Option<f64> {
    runs.iter().find_map(|run| {
        run.windows(2).find_map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            (a.x <= station && station <= b.x && b.x > a.x)
                .then(|| a.y + (b.y - a.y) * (station - a.x) / (b.x - a.x))
        })
    })
}

/// Frame, grid, profiles and axis annotation of a profile view, with the
/// view style's station and elevation formats.
fn profile_view_text(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let frame = data.profile_view_frame()?;
    for run in profile_view(data, document)?.split(|p| p[0].is_nan()) {
        if run.len() > 1 {
            line(out, run.to_vec());
        }
    }
    let view_style = data
        .decode()
        .handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_PROFILE_VIEW_STYLE");
    let formats: Vec<Vec<LabelPart>> = view_style
        .map(|s| s.inline_strings())
        .unwrap_or_default()
        .iter()
        .filter(|s| s.contains("<["))
        .map(|s| parse_format(s))
        .collect();
    let field_options = |wanted: u32| {
        formats.iter().flatten().find_map(|part| match part {
            LabelPart::Field { property, options } if *property == wanted => Some(options.clone()),
            _ => None,
        })
    };
    let station_options = field_options(property::STATION).unwrap_or_default();
    let elevation_options = field_options(ELEVATION).unwrap_or_default();
    let height = DEFAULT_PLOTTED_HEIGHT * drawing_scale(document);
    let label = |out: &mut Decoded, at: Vector2, value: String| {
        out.texts.push(ProxyText {
            position: [at.x, at.y, 0.0],
            height,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            text: value,
            font: String::new(),
            style: ProxyStyle::default(),
        });
    };
    let (s0, s1, e0, e1) = (
        frame.station_min,
        frame.station_max,
        frame.elevation_min,
        frame.elevation_max,
    );
    for station in grid(s0, s1, 100.0).chain([s0, s1]) {
        let q = frame.to_drawing(station, e0);
        let value = LabelPart::format_value(&station_options, station);
        label(out, Vector2::new(q.x - height, q.y - 2.0 * height), value);
    }
    for elevation in grid(e0, e1, 10.0).chain([e0, e1]) {
        let value = LabelPart::format_value(&elevation_options, elevation);
        let q = frame.to_drawing(s0, elevation);
        label(out, Vector2::new(q.x - (value.len() as f64 + 1.0) * height, q.y - height / 2.0), value.clone());
        let r = frame.to_drawing(s1, elevation);
        label(out, Vector2::new(r.x + height, r.y - height / 2.0), value);
    }
    Some(())
}
