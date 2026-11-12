//! Civil 3D entities, drawn from the geometry their class body stores.
//!
//! The drawing's proxy graphics for these classes are only a bounding box,
//! so each kind is rebuilt from the decoded record.

use document::classes::arch::ArchEntityData;
use document::classes::arch::ArchObject;
use document::classes::civil::{
    display_station, parse_format, property, AlignmentPiece, AssemblyLayout, CivilKind, DisplayComponent, LabelPart, LabelText,
    ProfilePiece, ProfileViewFrame, TinSurface,
};
use document::objects::ObjectType;
use document::entities::{EntityType, ExtendedEntityData};
use document::types::Handle;

use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::convert::proxy_graphics::{
    Decoded, ProxyColor, ProxyPolyline, ProxyStyle, ProxyText, LT_BYBLOCK, LT_BYLAYER,
};
use document::types::Vector2;

const NAN: [f64; 3] = [f64::NAN; 3];

type Key = (usize, usize);

/// Decoded geometry of the slow kinds (TIN, corridor), keyed by the record
/// bytes it came from; an edited record has new bytes and decodes again.
// ponytail: never evicts; bound it if drawings are edited heavily in one session.
fn cached<T: Clone + Default + Send + Sync + 'static>(
    data: &ArchEntityData,
    tag: u64,
    build: impl FnOnce(&ArchEntityData) -> Option<T>,
) -> T {
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(Key, u64), Arc<dyn Any + Send + Sync>>>> = OnceLock::new();
    let Some(raw) = data.raw_dwg_data.as_ref() else {
        return build(data).unwrap_or_default();
    };
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        ((hasher.finish() as usize, raw.len()), tag)
    };
    let cache = CACHE.get_or_init(Default::default);
    let hit = cache.lock().ok().and_then(|c| c.get(&key).cloned());
    if let Some(value) = hit.and_then(|hit| hit.downcast_ref::<T>().cloned()) {
        return value;
    }
    let value = build(data).unwrap_or_default();
    if let Ok(mut c) = cache.lock() {
        c.insert(key, Arc::new(value.clone()));
    }
    value
}

/// Every line a Civil 3D object draws, for picking and extents.
pub(crate) fn render(data: &ArchEntityData, handle: u64, document: &document::Document) -> Option<RenderEntity> {
    // A drawn display cache is what the object draws; it is picked as a proxy.
    let cached = document
        .get_entity(Handle::new(handle))
        .and_then(|e| e.common().graphic_data.as_deref())
        .is_some_and(crate::scene::convert::tess::civil_drawn_cache);
    if cached {
        return None;
    }
    let drawn = decoded(data, handle, document)?;
    let points = chains(drawn.polylines.into_iter().map(|p| p.points));
    (points.len() >= 2).then(|| RenderEntity {
        object: RenderObject::Lines(points),
        snap_pts: Vec::new(),
        tangent_geoms: Vec::new(),
        key_vertices: Vec::new(),
        fill_tris: Vec::new(),
        pick_tris: Vec::new(),
    })
}

/// Display components of the first `class` style among `handles`.
fn style_components(document: &document::Document, handles: &[u64], class: &str) -> Option<Vec<DisplayComponent>> {
    let style = handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == class)?;
    Some(style.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h)))
}

/// How component `index` of a style draws, `None` when the style hides it.
/// Without a style the object's own properties apply. A component on layer
/// 0 draws on the object's layer, as Civil 3D does.
fn component(document: &document::Document, components: Option<&[DisplayComponent]>, index: usize) -> Option<ProxyStyle> {
    use document::types::Color;
    let Some(components) = components else {
        return Some(ProxyStyle::default());
    };
    let c = components.get(index)?;
    if !c.visible {
        return None;
    }
    let layer = document
        .layers
        .iter()
        .position(|l| l.handle.value() == c.layer && l.name != "0")
        .map(|i| i as u32);
    let by = |name: &str, wanted: &str| name.eq_ignore_ascii_case(wanted);
    let linetype = match document.line_types.iter().find(|lt| lt.handle.value() == c.linetype) {
        Some(lt) if by(&lt.name, "ByBlock") => LT_BYBLOCK,
        Some(lt) if !by(&lt.name, "ByLayer") => document
            .line_types
            .iter()
            .filter(|t| !by(&t.name, "ByLayer") && !by(&t.name, "ByBlock"))
            .position(|t| t.handle == lt.handle)
            .map_or(LT_BYLAYER, |i| i as u32),
        _ => LT_BYLAYER,
    };
    let color = match c.color {
        Color::ByLayer => ProxyColor::ByLayer,
        Color::ByBlock | Color::None => ProxyColor::ByBlock,
        Color::Index(i) => ProxyColor::Aci(i),
        Color::Rgb { r, g, b } => ProxyColor::Rgb(r, g, b),
    };
    Some(ProxyStyle {
        color,
        layer,
        linetype,
        lineweight: c.lineweight,
    })
}

/// Lines drawn with a component's style; nothing when it is hidden.
fn styled(out: &mut Decoded, style: Option<ProxyStyle>, points: Vec<[f64; 3]>) {
    if let Some(style) = style {
        if points.len() > 1 {
            out.polylines.push(ProxyPolyline { points, style });
        }
    }
}

/// Surface style components in plan view (`SurfaceDisplayStyleType`).
const SURFACE_TRIANGLES: usize = 1;
const SURFACE_BOUNDARY: usize = 2;
const SURFACE_MAJOR_CONTOUR: usize = 3;
const SURFACE_MINOR_CONTOUR: usize = 4;

fn tin_surface(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) {
    let handles = data.decode().handles;
    let components = style_components(document, &handles, "AECC_SURFACE_STYLE");
    let components = components.as_deref();
    let draw = |out: &mut Decoded, index: usize, tag: u64, build: &dyn Fn(&TinSurface) -> Vec<[f64; 3]>| {
        if let Some(style) = component(document, components, index) {
            styled(out, Some(style), cached(data, tag, |d| Some(build(&d.tin_surface()?))));
        }
    };
    draw(out, SURFACE_TRIANGLES, 1, &tin_edges);
    draw(out, SURFACE_BOUNDARY, 2, &|surface| {
        chains(
            surface
                .boundary()
                .into_iter()
                .map(|run| run.into_iter().map(|p| [p.x, p.y, p.z]).collect()),
        )
    });
    let intervals = handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_SURFACE_STYLE")
        .and_then(|style| style.surface_contour_intervals());
    if let Some((minor, major)) = intervals {
        let is_major = move |z: f64| ((z / major).round() * major - z).abs() < 1e-6 * major.max(1.0);
        let contours = move |surface: &TinSurface, want_major: bool| {
            chains(
                surface
                    .contours(minor)
                    .into_iter()
                    .filter(|(z, _)| is_major(*z) == want_major)
                    .map(|(_, run)| run.into_iter().map(|p| [p.x, p.y, p.z]).collect()),
            )
        };
        let tag = |kind: u64, interval: f64| kind ^ interval.to_bits().rotate_left(8);
        draw(out, SURFACE_MAJOR_CONTOUR, tag(3, major) ^ minor.to_bits(), &|s| contours(s, true));
        draw(out, SURFACE_MINOR_CONTOUR, tag(4, minor) ^ major.to_bits(), &|s| contours(s, false));
    }
}

/// Feature lines of a corridor, each code's group drawn with the feature
/// line style the corridor gives that code.
fn corridor(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) {
    let baselines: Vec<Vec<Vec<[f64; 3]>>> = cached(data, 0, |data| {
        Some(
            data.corridor_baselines()
                .into_iter()
                .map(|groups| {
                    groups
                        .into_iter()
                        .map(|group| {
                            chains(
                                group
                                    .into_iter()
                                    .map(|run| run.into_iter().map(|(_, p)| [p.x, p.y, p.z]).collect()),
                            )
                        })
                        .collect()
                })
                .collect(),
        )
    });
    // Body handles: a null, the corridor style, the code set style, then
    // one feature line style per code, in code order.
    let handles = data.decode().handles;
    let styles: Vec<&ArchObject> = handles
        .iter()
        .skip(3)
        .map_while(|h| object_arch(document, *h).filter(|o| o.dxf_name == "AECC_FEATURELINE_STYLE"))
        .collect();
    for groups in baselines {
        // ponytail: a code without feature lines has no group, so a
        // baseline's groups and the codes pair up only when their counts
        // match; otherwise the corridor keeps its own properties. Pair them
        // by code once the corridor's assembly codes are decoded.
        let matched = styles.len() == groups.len();
        for (index, points) in groups.into_iter().enumerate() {
            let style = if matched {
                let components = styles[index].display_components(|h| document.layers.iter().any(|l| l.handle.value() == h));
                // Feature line style component 0: the line in plan.
                component(document, Some(&components), 0)
            } else {
                Some(ProxyStyle::default())
            };
            styled(out, style, points);
        }
    }
}

/// Alignment style components in plan view (`AlignmentDisplayStyleType`),
/// drawn only outside the alignment's masked station ranges.
fn alignment(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) {
    let components = style_components(document, &data.decode().handles, "AECC_ALIGNMENT_STYLE");
    let masks = data.alignment_masked_ranges();
    let mut station = data.alignment_start_station().unwrap_or(0.0);
    for (piece, points) in data.alignment_pieces() {
        let index = match piece {
            AlignmentPiece::Line => 0,
            AlignmentPiece::Curve => 1,
            AlignmentPiece::Spiral => 2,
        };
        let style = component(document, components.as_deref(), index);
        for run in unmasked(&points, station, &masks) {
            styled(out, style, run.into_iter().map(|p| [p.x, p.y, 0.0]).collect());
        }
        station += points.windows(2).map(|w| w[0].distance(&w[1])).sum::<f64>();
    }
}

/// Whether `station` lies in a masked range.
fn masked(station: f64, masks: &[(f64, f64)]) -> bool {
    masks.iter().any(|&(from, to)| station > from && station < to)
}

/// Parts of a polyline starting at station `start` outside `masks`, cut
/// exactly at the range ends.
fn unmasked(points: &[Vector2], start: f64, masks: &[(f64, f64)]) -> Vec<Vec<Vector2>> {
    if masks.is_empty() {
        return vec![points.to_vec()];
    }
    let mut out = Vec::new();
    let mut current: Vec<Vector2> = Vec::new();
    let mut station = start;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(&b);
        // Split this segment at every range end inside it.
        let mut cuts: Vec<f64> = masks
            .iter()
            .flat_map(|&(from, to)| [from, to])
            .filter(|&s| s > station && s < station + length)
            .collect();
        cuts.sort_by(f64::total_cmp);
        let mut s0 = station;
        let mut p0 = a;
        for s1 in cuts.into_iter().chain([station + length]) {
            let p1 = if length > 0.0 { a + (b - a) * ((s1 - station) / length) } else { b };
            if masked((s0 + s1) / 2.0, masks) {
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
            } else {
                if current.is_empty() {
                    current.push(p0);
                }
                current.push(p1);
            }
            s0 = s1;
            p0 = p1;
        }
        station += length;
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// A marker style's point symbol at `at` (`Marker::kind` bits: low 2 plus,
/// 3 cross, 4 tick; +32 circle, +64 square).
fn marker(out: &mut Decoded, style: Option<ProxyStyle>, at: Vector2, symbol: &document::classes::civil::Marker, scale: f64) {
    let size = symbol.size * if symbol.absolute { 1.0 } else { scale };
    let p = |x: f64, y: f64| [at.x + x, at.y + y, 0.0];
    if symbol.kind & 32 != 0 {
        let r = size / 2.0;
        styled(out, style, (0..=48).map(|i| {
            let a = i as f64 / 48.0 * std::f64::consts::TAU;
            p(r * a.cos(), r * a.sin())
        }).collect());
    }
    if symbol.kind & 64 != 0 {
        let h = size / 2.0;
        styled(out, style, vec![p(-h, -h), p(-h, h), p(h, h), p(h, -h), p(-h, -h)]);
    }
    match symbol.kind & 31 {
        2 => {
            styled(out, style, vec![p(-size, 0.0), p(size, 0.0)]);
            styled(out, style, vec![p(0.0, -size), p(0.0, size)]);
        }
        3 => {
            let d = size * std::f64::consts::FRAC_1_SQRT_2;
            styled(out, style, vec![p(-d, -d), p(d, d)]);
            styled(out, style, vec![p(-d, d), p(d, -d)]);
        }
        4 => styled(out, style, vec![p(0.0, 0.0), p(0.0, size)]),
        _ => {}
    }
}

/// The marker style object's symbol and its first component's style.
fn marker_style(document: &document::Document, handle: u64) -> Option<(document::classes::civil::Marker, Option<ProxyStyle>)> {
    let object = object_arch(document, handle)?;
    let symbol = object.marker()?;
    let components = object.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h));
    Some((symbol, component(document, Some(&components), 0)))
}

/// An assembly's baseline marker, baseline line and assembly marker, from
/// its style's marker styles (baseline, offset, baseline line, offset line,
/// assembly).
fn assembly(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let origin = data.assembly_layout()?.origin;
    let at = Vector2::new(origin.x, origin.y);
    let style = data
        .decode()
        .handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_ASSEMBLY_STYLE")?;
    let markers = style.decode().handles;
    let scale = drawing_scale(document);
    if let Some((symbol, line_style)) = markers.get(2).and_then(|h| marker_style(document, *h)) {
        let half = symbol.size * if symbol.absolute { 1.0 } else { scale } / 2.0;
        styled(out, line_style, vec![[at.x, at.y + half, 0.0], [at.x, at.y - half, 0.0]]);
    }
    for index in [0, 4] {
        if let Some((symbol, marker_layer)) = markers.get(index).and_then(|h| marker_style(document, *h)) {
            marker(out, marker_layer, at, &symbol, scale);
        }
    }
    Some(())
}

/// A subassembly's links, area borders and point markers at its attachment
/// point, each styled by its code through the code set style: the first
/// code the set lists picks the style; codes the set does not list, or no
/// codes, take the set's defaults.
fn subassembly(data: &ArchEntityData, handle: u64, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let shape = data.subassembly_shape()?;
    let assembly = arch_of(document, data.subassembly_assembly()?, CivilKind::Assembly)?;
    let layout = assembly.assembly_layout()?;
    let origin = subassembly_origin(document, &layout, handle, layout.attachments.len())?;
    let code_set = data
        .decode()
        .handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_ROADWAY_STYLE_SET");
    let entries = code_set.map(|c| c.code_set()).unwrap_or_default();
    let defaults = code_set.map(|c| c.code_set_defaults()).unwrap_or_default();
    // Every code the set lists draws with its own style; codes it does not
    // list, or no codes, take a default (uncoded marker, link, shape, then
    // the same for unlisted codes).
    let pick = |codes: &[String], class: &str, uncoded: usize, unlisted: usize| -> Vec<u64> {
        if codes.is_empty() {
            return defaults.get(uncoded).copied().into_iter().collect();
        }
        let listed: Vec<u64> = codes
            .iter()
            .filter_map(|code| {
                entries
                    .iter()
                    .find(|(c, style)| c == code && object_arch(document, *style).is_some_and(|o| o.dxf_name == class))
                    .map(|(_, style)| *style)
            })
            .collect();
        if listed.is_empty() {
            defaults.get(unlisted).copied().into_iter().collect()
        } else {
            listed
        }
    };
    let first_component = |style: u64, index: usize| {
        let object = object_arch(document, style)?;
        let components = object.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h));
        component(document, Some(&components), index)
    };
    let point = |i: u32| {
        let p = origin + shape.points[i as usize];
        [p.x, p.y, 0.0]
    };
    for (index, link) in shape.links.iter().enumerate() {
        let codes = shape.link_codes.get(index).map(Vec::as_slice).unwrap_or(&[]);
        for style in pick(codes, "AECC_ROADWAYLINK_STYLE", 1, 4) {
            styled(out, first_component(style, 0), link.iter().map(|&i| point(i)).collect());
        }
    }
    for (index, area) in shape.areas.iter().enumerate() {
        let codes = shape.area_codes.get(index).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(border) = area_border(&shape, area) {
            for style in pick(codes, "AECC_ROADWAYSHAPE_STYLE", 2, 5) {
                styled(out, first_component(style, 0), border.iter().map(|&i| point(i)).collect());
            }
        }
    }
    let scale = drawing_scale(document);
    for (index, p) in shape.points.iter().enumerate() {
        let codes = shape.point_codes.get(index).map(Vec::as_slice).unwrap_or(&[]);
        for style in pick(codes, "AECC_ROADWAYMARKER_STYLE", 0, 3) {
            if let Some((symbol, layer_style)) = marker_style(document, style) {
                marker(out, layer_style, origin + *p, &symbol, scale);
            }
        }
    }
    Some(())
}

/// Point indices around an area, chaining its links end to end.
fn area_border(shape: &document::classes::civil::SubassemblyShape, area: &[u32]) -> Option<Vec<u32>> {
    let ends = |link: u32| {
        let l = shape.links.get(link as usize)?;
        Some((*l.first()?, *l.last()?))
    };
    let mut remaining: Vec<(u32, u32)> = area.iter().map(|&l| ends(l)).collect::<Option<_>>()?;
    let (a, b) = remaining.remove(0);
    let mut ring = vec![a, b];
    while !remaining.is_empty() {
        let tail = *ring.last()?;
        let position = remaining.iter().position(|&(p, q)| p == tail || q == tail)?;
        let (p, q) = remaining.remove(position);
        ring.push(if p == tail { q } else { p });
    }
    Some(ring)
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

/// What a Civil 3D object draws, styled by its style's display
/// components. `None` for classes this module does not draw; an object its
/// style hides draws nothing.
pub(crate) fn decoded(data: &ArchEntityData, handle: u64, document: &document::Document) -> Option<Decoded> {
    let mut out = Decoded::default();
    match data.civil_kind()? {
        CivilKind::TinSurface => tin_surface(data, document, &mut out),
        CivilKind::Alignment => alignment(data, document, &mut out),
        CivilKind::ProfileView => {
            profile_view(data, document, &mut out);
        }
        CivilKind::Label => {
            label_group(data, document, &mut out);
        }
        CivilKind::SampleLineGroup => {
            let handles = data.decode().handles;
            let group = style_components(document, &handles, "AECC_SAMPLE_LINE_GROUP_STYLE");
            let lines = style_components(document, &handles, "AECC_SAMPLE_LINE_STYLE");
            // Plan components: the group style's lines, then the sample line
            // style's `SampleLineDisplayStyleType::Lines`.
            if component(document, group.as_deref(), 0).is_some() {
                styled(&mut out, component(document, lines.as_deref(), 0), sample_line_points(data, document).unwrap_or_default());
            }
        }
        CivilKind::Corridor => corridor(data, document, &mut out),
        CivilKind::Assembly => {
            assembly(data, document, &mut out);
        }
        CivilKind::Subassembly => {
            subassembly(data, handle, document, &mut out);
        }
        CivilKind::Other if data.dxf_name == "AECC_PIPE" => pipe(data, document, &mut out)?,
        CivilKind::Other if data.dxf_name == "AECC_FEATURE_LINE" => feature_line(data, handle, document, &mut out)?,
        _ => return None,
    }
    Some(out)
}

/// Stations as Civil 3D shows them along `alignment`: past a station
/// equation they run on from its station ahead. Without an alignment, raw.
fn shown_stations(alignment: Option<&ArchEntityData>) -> impl Fn(f64) -> f64 {
    let equations = alignment.map(|a| a.alignment_station_equations()).unwrap_or_default();
    move |raw| display_station(&equations, raw)
}

/// The alignment among `handles`.
fn alignment_among<'a>(document: &'a document::Document, handles: &[u64]) -> Option<&'a ArchEntityData> {
    handles.iter().find_map(|h| arch_of(document, *h, CivilKind::Alignment))
}

fn line(out: &mut Decoded, points: Vec<[f64; 3]>) {
    out.polylines.push(ProxyPolyline {
        points,
        style: ProxyStyle::default(),
    });
}

/// A label style as drawn: its layer, text components and tick blocks.
struct LabelStyle {
    layer: Option<u32>,
    components: Vec<LabelText>,
    /// `(block record handle, plotted size)` of each tick component.
    ticks: Vec<(u64, f64)>,
    /// `(plotted length, angle to the anchor's direction)` of each line
    /// component.
    lines: Vec<(f64, f64)>,
}

/// A label's own line components draw on the label's layer.
fn label_line_style(layer: Option<u32>) -> ProxyStyle {
    ProxyStyle {
        layer,
        color: ProxyColor::ByLayer,
        linetype: LT_BYLAYER,
        ..ProxyStyle::default()
    }
}

/// Lines of a tick block placed at `at`, turned by `angle` and scaled to
/// `size` drawing units, on the label's layer.
fn tick(out: &mut Decoded, document: &document::Document, layer: Option<u32>, block: u64, at: Vector2, angle: f64, size: f64) {
    let Some(record) = document.block_records.iter().find(|b| b.handle.value() == block) else {
        return;
    };
    // The tick block is scaled so its larger extent is `size`.
    let mut low = [f64::MAX; 2];
    let mut high = [f64::MIN; 2];
    let mut grow = |x: f64, y: f64, r: f64| {
        low = [low[0].min(x - r), low[1].min(y - r)];
        high = [high[0].max(x + r), high[1].max(y + r)];
    };
    for entity in document.entities_in_block(&record.name) {
        match entity {
            EntityType::Line(l) => {
                grow(l.start.x, l.start.y, 0.0);
                grow(l.end.x, l.end.y, 0.0);
            }
            EntityType::Circle(c) => grow(c.center.x, c.center.y, c.radius),
            EntityType::LwPolyline(p) => p.vertices.iter().for_each(|v| grow(v.location.x, v.location.y, 0.0)),
            _ => {}
        }
    }
    let extent = (high[0] - low[0]).max(high[1] - low[1]);
    let size = if extent > 1e-12 { size / extent } else { size };
    let (sin, cos) = angle.sin_cos();
    let place = |x: f64, y: f64| [at.x + (x * cos - y * sin) * size, at.y + (x * sin + y * cos) * size, 0.0];
    let style = label_line_style(layer);
    for entity in document.entities_in_block(&record.name) {
        let points = match entity {
            EntityType::Line(l) => vec![place(l.start.x, l.start.y), place(l.end.x, l.end.y)],
            EntityType::Circle(c) => (0..=64)
                .map(|i| {
                    let a = i as f64 / 64.0 * std::f64::consts::TAU;
                    place(c.center.x + c.radius * a.cos(), c.center.y + c.radius * a.sin())
                })
                .collect(),
            EntityType::LwPolyline(p) => {
                let mut points: Vec<[f64; 3]> = p.vertices.iter().map(|v| place(v.location.x, v.location.y)).collect();
                if p.is_closed {
                    if let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                }
                points
            }
            _ => continue,
        };
        styled(out, Some(style), points);
    }
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
    let mut seen = Vec::new();
    let ticks = handles
        .iter()
        .filter(|h| {
            let fresh = !seen.contains(*h);
            seen.push(**h);
            fresh
        })
        .filter_map(|h| object_arch(document, *h)?.label_tick())
        .collect();
    let mut seen = Vec::new();
    let lines = handles
        .iter()
        .filter(|h| {
            let fresh = !seen.contains(*h);
            seen.push(**h);
            fresh
        })
        .filter_map(|h| object_arch(document, *h)?.label_line())
        .collect();
    LabelStyle { layer, components, ticks, lines }
}

/// Stroke font of Civil 3D label text: the drawing's Standard text style
/// font.
// ponytail: every component here uses Standard; read each text component's
// own style once its reference is decoded.
fn label_font(document: &document::Document) -> String {
    document
        .text_styles
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case("Standard"))
        .map(|s| s.font_file.clone())
        .unwrap_or_default()
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

/// Angle past which label text is turned around to stay readable: text
/// reading between it and half a turn further reads the other way.
// ponytail: 110°, the value in this drawing's label styles; read each
// style's own once its field is decoded.
const READABILITY_BIAS: f64 = 110.0 * std::f64::consts::PI / 180.0;

/// MTEXT line spacing at the default factor, in text heights.
const LINE_SPACING: f64 = 5.0 / 3.0;

/// Place a label's text components at `anchor`, in the frame of the
/// anchor's direction `frame` (radians): each component sits at its offset
/// along and across that direction, turned by its own rotation, held by its
/// attachment point and border gap, and turned around when it would read
/// upside down.
fn place(
    out: &mut Decoded,
    document: &document::Document,
    style: &LabelStyle,
    anchor: Vector2,
    frame: f64,
    value: &dyn Fn(u32, &[u32]) -> Option<String>,
) {
    let scale = drawing_scale(document);
    let (fs, fc) = frame.sin_cos();
    for component in &style.components {
        let Some(lines) = component_lines(component, value) else {
            continue;
        };
        let lines: Vec<String> = lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let height = component.height.unwrap_or(DEFAULT_PLOTTED_HEIGHT) * scale;
        let (x, y) = (component.x_offset * scale, component.y_offset * scale);
        let mut at = anchor + Vector2::new(x * fc - y * fs, x * fs + y * fc);
        let mut angle = frame + component.rotation;
        // Fractions of the text box: 0 left / bottom, 1 right / top.
        let column = f64::from(component.attachment % 3) * 0.5;
        let row = 1.0 - f64::from(component.attachment / 3) * 0.5;
        let gap = component.gap * scale;
        let (u, v) = (Vector2::new(angle.cos(), angle.sin()), Vector2::new(-angle.sin(), angle.cos()));
        at = at + u * (gap * (1.0 - 2.0 * column)) + v * (gap * (1.0 - 2.0 * row));
        let (mut column, mut row) = (column, row);
        let reading = angle.rem_euclid(std::f64::consts::TAU);
        if reading > READABILITY_BIAS && reading <= READABILITY_BIAS + std::f64::consts::PI {
            // Same box, read from the other end.
            angle += std::f64::consts::PI;
            column = 1.0 - column;
            row = 1.0 - row;
        }
        let v = Vector2::new(-angle.sin(), angle.cos());
        let spacing = height * LINE_SPACING;
        let block = height + spacing * (lines.len() - 1) as f64;
        // Each line hangs from its own top; the block's top is where the
        // attachment point puts it.
        let top = at + v * (block * (1.0 - row));
        for (i, line_text) in lines.into_iter().enumerate() {
            let p = top - v * (spacing * i as f64);
            out.texts.push(ProxyText {
                position: [p.x, p.y, 0.0],
                height,
                rotation: angle,
                width_factor: 1.0,
                oblique: 0.0,
                text: line_text,
                font: label_font(document),
                style: ProxyStyle {
                    layer: style.layer,
                    ..ProxyStyle::default()
                },
                align: [column, 1.0],
            });
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

/// Millimetres in one unit a part definition field names.
fn part_unit_mm(unit: &str) -> Option<f64> {
    Some(match unit.trim().to_ascii_lowercase().as_str() {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "inch" | "in" => 25.4,
        "ft" | "foot" | "feet" => 304.8,
        _ => return None,
    })
}

/// Number fields of the part definition among `handles`, by context name,
/// in drawing units where the field has a unit.
fn part_size(document: &document::Document, handles: &[u64]) -> std::collections::HashMap<String, f64> {
    let drawing_mm = crate::app::properties::insunits_to_mm(document.header.insertion_units);
    handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_NETWORK_PART_DEF")
        .map(|part| {
            part.network_part_fields()
                .into_iter()
                .filter_map(|f| {
                    let value = match (part_unit_mm(&f.unit), drawing_mm) {
                        (Some(unit), Some(drawing)) => f.value * unit / drawing,
                        (None, _) if f.unit.is_empty() => f.value,
                        // ponytail: a unitless drawing keeps the part's own
                        // numbers; set INSUNITS to scale them.
                        _ => f.value,
                    };
                    Some((f.context, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Where a structure stands in plan, for cutting the pipes that enter it.
#[derive(Clone, Copy)]
enum Footprint {
    /// A cylinder: insertion point and half its bounding diameter.
    Circle(Vector2, f64),
    /// Any other part: the extents its cached preview records.
    // ponytail: extents are axis-aligned, so a rotated box structure cuts
    // its pipes short of its walls; decode the part's rotation to fit them.
    Box(Vector2, Vector2),
}

impl Footprint {
    fn contains(&self, p: Vector2) -> bool {
        match *self {
            Footprint::Circle(c, r) => p.distance(&c) < r - 1e-9,
            Footprint::Box(lo, hi) => p.x > lo.x + 1e-9 && p.x < hi.x - 1e-9 && p.y > lo.y + 1e-9 && p.y < hi.y - 1e-9,
        }
    }

    /// Parameters along `a + t d` where the segment is inside, if any.
    fn inside(&self, a: Vector2, d: Vector2) -> Option<(f64, f64)> {
        match *self {
            Footprint::Circle(c, r) => {
                let len2 = d.x * d.x + d.y * d.y;
                let f = a - c;
                let bq = f.x * d.x + f.y * d.y;
                let disc = bq * bq - len2 * (f.x * f.x + f.y * f.y - r * r);
                (disc > 0.0).then(|| ((-bq - disc.sqrt()) / len2, (-bq + disc.sqrt()) / len2))
            }
            Footprint::Box(lo, hi) => {
                let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
                for (p, dv, l, h) in [(a.x, d.x, lo.x, hi.x), (a.y, d.y, lo.y, hi.y)] {
                    if dv.abs() < 1e-15 {
                        if p <= l || p >= h {
                            return None;
                        }
                    } else {
                        let (u, v) = ((l - p) / dv, (h - p) / dv);
                        t0 = t0.max(u.min(v));
                        t1 = t1.min(u.max(v));
                    }
                }
                (t1 > t0).then_some((t0, t1))
            }
        }
    }
}

fn structure_footprint(document: &document::Document, handle: u64) -> Option<Footprint> {
    let EntityType::Extended(entity) = document.get_entity(Handle::new(handle))? else {
        return None;
    };
    let ExtendedEntityData::Arch(data) = &entity.data else {
        return None;
    };
    let size = part_size(document, &data.decode().handles);
    match size.get("StructDiameter") {
        Some(&diameter) if diameter > 0.0 => {
            let at = data.structure_position()?;
            Some(Footprint::Circle(Vector2::new(at.x, at.y), diameter / 2.0))
        }
        _ => {
            let mut lo = Vector2::new(f64::INFINITY, f64::INFINITY);
            let mut hi = Vector2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in data.profiles.iter().flatten() {
                lo = Vector2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vector2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            (lo.x < hi.x && lo.y < hi.y).then_some(Footprint::Box(lo, hi))
        }
    }
}

/// Structures a pipe connects to: each of its connectors points at the
/// structure's connector, which points at the structure. A connector's
/// handles end with its partner and its owner (after one null in Civil 3D
/// 2013 drawings, two from 2018).
fn pipe_structures(document: &document::Document, handles: &[u64]) -> Vec<Footprint> {
    let connector = |h: u64| {
        let EntityType::Extended(entity) = document.get_entity(Handle::new(h))? else {
            return None;
        };
        let ExtendedEntityData::Arch(data) = &entity.data else {
            return None;
        };
        (data.dxf_name == "AECC_NETWORK_PART_CONNECTOR").then(|| data.decode().handles)
    };
    handles
        .iter()
        .filter_map(|h| connector(*h))
        .filter_map(|own| {
            let partner = *own.get(own.len().checked_sub(2)?)?;
            (partner != 0).then_some(())?;
            connector(partner)?.last().copied()
        })
        .filter_map(|structure| structure_footprint(document, structure))
        .collect()
}

/// Parts of the segment `a`-`b` outside every footprint.
fn outside_footprints(a: Vector2, b: Vector2, footprints: &[Footprint]) -> Vec<(Vector2, Vector2)> {
    let d = b - a;
    if d.x * d.x + d.y * d.y <= 0.0 {
        return Vec::new();
    }
    let mut keep = vec![(0.0f64, 1.0f64)];
    for footprint in footprints {
        let Some((t0, t1)) = footprint.inside(a, d) else {
            continue;
        };
        keep = keep
            .into_iter()
            .flat_map(|(s, e)| {
                let mut parts = Vec::new();
                if t0 > s {
                    parts.push((s, t0.min(e)));
                }
                if t1 < e {
                    parts.push((t1.max(s), e));
                }
                parts.into_iter().filter(|(s, e)| e - s > 1e-9)
            })
            .collect();
    }
    keep.into_iter().map(|(s, e)| (a + d * s, a + d * e)).collect()
}

/// Pipe style components in plan, in the order the style class lists them.
const PIPE_CENTERLINE: usize = 0;
const PIPE_INSIDE_WALLS: usize = 1;
const PIPE_OUTSIDE_WALLS: usize = 2;
const PIPE_END_LINE: usize = 3;

/// A Civil 3D pipe in plan: centerline, inside and outside walls and end
/// lines, each styled by its pipe style, cut back where the pipe enters a
/// structure. `None` when its ends or size do not read.
fn pipe(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let (a3, b3) = data.pipe_ends()?;
    let (a, b) = (Vector2::new(a3.x, a3.y), Vector2::new(b3.x, b3.y));
    let handles = data.decode().handles;
    let size = part_size(document, &handles);
    let inner = size.get("PipeInnerDiameter").or_else(|| size.get("BodyGeometry_Diameter")).copied()?;
    let outer = inner + 2.0 * size.get("WallThickness").copied().unwrap_or(0.0);
    let u = unit2(b - a)?;
    let normal = Vector2::new(-u.y, u.x);
    let components = style_components(document, &handles, "AECC_PIPE_STYLE");
    let structures = pipe_structures(document, &handles);
    let inside_any = |p: Vector2| structures.iter().any(|f| f.contains(p));
    let mut draw = |index: usize, offsets: &[f64]| {
        let style = component(document, components.as_deref(), index);
        for &o in offsets {
            for (p, q) in outside_footprints(a + normal * o, b + normal * o, &structures) {
                styled(out, style, vec![[p.x, p.y, 0.0], [q.x, q.y, 0.0]]);
            }
        }
    };
    draw(PIPE_CENTERLINE, &[0.0]);
    draw(PIPE_INSIDE_WALLS, &[inner / 2.0, -inner / 2.0]);
    draw(PIPE_OUTSIDE_WALLS, &[outer / 2.0, -outer / 2.0]);
    let style = component(document, components.as_deref(), PIPE_END_LINE);
    for end in [a, b] {
        if !inside_any(end) {
            let (p, q) = (end + normal * (outer / 2.0), end - normal * (outer / 2.0));
            styled(out, style, vec![[p.x, p.y, 0.0], [q.x, q.y, 0.0]]);
        }
    }
    Some(())
}

/// Plan component of a feature line style that draws the line.
const FEATURE_LINE: usize = 0;

/// A feature line keeps no geometry of its own: it is an edge of its site's
/// topology, with the elevations in the nodes.
fn feature_line(data: &ArchEntityData, handle: u64, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let container = object_arch(document, data.feature_line_container()?)?;
    let members = container.site_members();
    let topology = container.topology()?;
    if members.len() != topology.edges.len() {
        return None;
    }
    let index = members.iter().position(|h| *h == handle)?;
    let components = style_components(document, &data.decode().handles, "AECC_FEATURELINE_STYLE");
    let points = topology.edge_points(index).into_iter().map(|p| [p.x, p.y, p.z]).collect();
    styled(out, component(document, components.as_deref(), FEATURE_LINE), points);
    Some(())
}

fn unit2(v: Vector2) -> Option<Vector2> {
    let l = v.x.hypot(v.y);
    (l > 1e-12).then(|| Vector2::new(v.x / l, v.y / l))
}

/// Point and unit direction at `distance` along a polyline.
fn point_at(run: &[Vector2], distance: f64) -> Option<(Vector2, Vector2)> {
    // A station the record stores may pass the polyline's end by the
    // rounding of its curves; it still names the end.
    const END_TOLERANCE: f64 = 1e-3;
    let mut left = distance;
    let mut last = None;
    for pair in run.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(&b);
        if length <= 0.0 {
            continue;
        }
        let d = (b - a) * (1.0 / length);
        if left <= length {
            return Some((a + d * left, d));
        }
        left -= length;
        last = Some((b, d));
    }
    last.filter(|_| left <= END_TOLERANCE)
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
            let _ = major;
            let shown = shown_stations(Some(alignment));
            for station in data.label_stations() {
                let Some((at, d)) = point_at(run, station - start) else {
                    continue;
                };
                let b = at;
                if let Some(style) = &style {
                    for &(block, size) in &style.ticks {
                        tick(out, document, style.layer, block, at, d.y.atan2(d.x), size * scale);
                    }
                }
                if let Some(style) = &style {
                    let value = |property: u32, options: &[u32]| {
                        (property == p::STATION).then(|| LabelPart::format_value(options, shown(station)))
                    };
                    place(out, document, style, b, d.y.atan2(d.x), &value);
                }
            }
        }
        "AECC_ALIGNMENT_GEOMPOINT_LABEL_GROUP" => {
            let alignment = alignment?;
            let style = style?;
            let start = alignment.alignment_start_station().unwrap_or(0.0);
            let runs = alignment.alignment_polylines();
            let run = runs.first()?;
            let scale = drawing_scale(document);
            let masks = alignment.alignment_masked_ranges();
            let shown = shown_stations(Some(alignment));
            for point in alignment.alignment_geometry_points() {
                if masked(start + point.distance, &masks) {
                    continue;
                }
                let Some((at, d)) = point_at(run, point.distance) else {
                    continue;
                };
                let heading = d.y.atan2(d.x);
                for &(block, size) in &style.ticks {
                    tick(out, document, style.layer, block, at, heading, size * scale);
                }
                // The text hangs from the end of the line component, in its
                // direction.
                let (tip, frame) = match style.lines.first() {
                    Some(&(length, angle)) => {
                        let frame = heading + angle;
                        let tip = at + Vector2::new(frame.cos(), frame.sin()) * (length * scale);
                        styled(out, Some(label_line_style(style.layer)), vec![[at.x, at.y, 0.0], [tip.x, tip.y, 0.0]]);
                        (tip, frame)
                    }
                    None => (at, heading),
                };
                let station = start + point.distance;
                let value = |property: u32, options: &[u32]| match property {
                    p::GEOMETRY_POINT => Some(point.name.to_string()),
                    p::STATION => Some(LabelPart::format_value(options, shown(station))),
                    _ => None,
                };
                place(out, document, &style, tip, frame, &value);
            }
        }
        "AECC_SAMPLELINE_LABELING" => {
            let group = handles
                .iter()
                .find_map(|h| arch_of(document, *h, CivilKind::SampleLineGroup))?;
            let style = style?;
            let template = group.sample_line_name_template().unwrap_or_default();
            let shown = shown_stations(alignment_among(document, &group.decode().handles));
            for (station, left, right) in sample_lines(group, document)? {
                let name: String = template
                    .iter()
                    .map(|part| match part {
                        LabelPart::Text(text) => text.clone(),
                        LabelPart::Field { options, .. } => LabelPart::format_value(options, shown(station)),
                        LabelPart::NewLine => String::new(),
                    })
                    .collect();
                let value = |property: u32, _: &[u32]| (property == p::SAMPLE_LINE_NAME).then(|| name.clone());
                let d = right - left;
                place(out, document, &style, left, d.y.atan2(d.x), &value);
            }
        }
        "AECC_STATION_ELEVATION_LABEL" => {
            let style = style?;
            let at = data.label_anchor()?;
            let (frame, view) = document
                .entities()
                .filter_map(|e| match e {
                    EntityType::Extended(x) => match &x.data {
                        ExtendedEntityData::Arch(view) => view.profile_view_frame().map(|f| (f, view)),
                        _ => None,
                    },
                    _ => None,
                })
                .find(|(f, _)| {
                    let top = f.to_drawing(f.station_max, f.elevation_max);
                    at.x >= f.origin.x && at.y >= f.origin.y && at.x <= top.x && at.y <= top.y
                })?;
            let shown = shown_stations(alignment_among(document, &view.decode().handles));
            let station = shown(frame.station_min + (at.x - frame.origin.x));
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
            // The view lists each band's style, then the alignment and the
            // band's two profiles.
            let band_at = view_handles.iter().position(|h| {
                object_arch(document, *h).is_some_and(|o| o.dxf_name == "AECC_PROFILE_VIEW_DATA_BAND_STYLE")
            })?;
            let band = object_arch(document, view_handles[band_at])?;
            let profiles: Vec<_> = view_handles[band_at + 1..]
                .iter()
                .filter_map(|h| arch_of(document, *h, CivilKind::Profile))
                .take(2)
                .collect();
            let eg = profiles.first().map(|p| p.profile_polylines()).unwrap_or_default();
            let fg = profiles.get(1).map(|p| p.profile_polylines()).unwrap_or_default();
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
            let scale = drawing_scale(document);
            // The band hangs below the view, its top on the view's bottom.
            let height = band.band_height().unwrap_or(0.0) * scale;
            let top = frame.origin.y;
            let band_components = band.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h));
            let ticks = component(document, Some(&band_components), BAND_MAJOR_TICKS);
            let shown = shown_stations(alignment_among(document, &view_handles));
            for station in data.label_stations() {
                let x = frame.to_drawing(station, frame.elevation_min).x;
                styled(out, ticks, vec![[x, top, 0.0], [x, top - height, 0.0]]);
                let value = |property: u32, options: &[u32]| match property {
                    p::STATION => Some(LabelPart::format_value(options, shown(station))),
                    EG_ELEVATION => elevation_at(&eg, station).map(|v| LabelPart::format_value(options, v)),
                    FG_ELEVATION => elevation_at(&fg, station).map(|v| LabelPart::format_value(options, v)),
                    _ => None,
                };
                // Each text hangs from the band's top, middle or bottom.
                for text in &style.components {
                    let y = match text.anchor_point {
                        Some(1) => top - height / 2.0,
                        Some(2) => top - height,
                        _ => top,
                    };
                    let single = LabelStyle {
                        layer: style.layer,
                        components: vec![text.clone()],
                        ticks: Vec::new(),
                        lines: Vec::new(),
                    };
                    place(out, document, &single, Vector2::new(x, y), 0.0, &value);
                }
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
            let shown = shown_stations(alignment_among(document, &profile.decode().handles));
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
                        // Half way along the straight part of the tangent:
                        // from the end of this PVI's curve to the start of
                        // the next one's.
                        let mid = if g.is_finite() {
                            let from = pvi.point.x + pvi.curve_length / 2.0;
                            let to = next.point.x - next.curve_length / 2.0;
                            let station = (from + to) / 2.0;
                            Vector2::new(station, pvi.point.y + g * (station - pvi.point.x))
                        } else {
                            (pvi.point + next.point) * 0.5
                        };
                        if !inside(mid) {
                            continue;
                        }
                        let (a, b) = (at(pvi.point), at(next.point));
                        let value = |property: u32, options: &[u32]| {
                            (property == p::TANGENT_GRADE).then(|| {
                                if g.is_finite() {
                                    LabelPart::format_value(options, g)
                                } else {
                                    "Vertical".to_string()
                                }
                            })
                        };
                        place(out, document, &style, at(mid), (b.y - a.y).atan2(b.x - a.x), &value);
                    }
                    "AECC_VALIGNMENT_PVI_LABEL_GROUP" => {
                        if !inside(pvi.point) {
                            continue;
                        }
                        let q = at(pvi.point);
                        let Some(&(length, angle)) = style.lines.first() else {
                            continue;
                        };
                        let top = q + Vector2::new(angle.cos(), angle.sin()) * (length * scale);
                        styled(out, Some(label_line_style(style.layer)), vec![[q.x, q.y, 0.0], [top.x, top.y, 0.0]]);
                        let value = |property: u32, options: &[u32]| match property {
                            p::PVI_STATION => Some(LabelPart::format_value(options, shown(pvi.point.x))),
                            p::PVI_ELEVATION => Some(LabelPart::format_value(options, pvi.point.y)),
                            _ => None,
                        };
                        place(out, document, &style, top, angle, &value);
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
                                p::PVI_STATION => shown(pvi.point.x),
                                p::PVI_ELEVATION => pvi.point.y,
                                p::BVC_STATION => shown(bvc.x),
                                p::BVC_ELEVATION => bvc.y,
                                p::EVC_STATION => shown(evc.x),
                                p::EVC_ELEVATION => evc.y,
                                p::HIGH_POINT_STATION | p::LOW_POINT_STATION => shown(extreme.x),
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
/// Elevations of a data band's first and second profile.
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

/// Profile view style components (`ProfileViewDisplayStyleType`, in
/// record order): per axis the line, its major and minor annotation and
/// ticks; then the grid.
const VIEW_LEFT: usize = 1;
const VIEW_RIGHT: usize = 7;
const VIEW_TOP: usize = 13;
const VIEW_BOTTOM: usize = 19;
const VIEW_GRID_HORIZONTAL_MAJOR: usize = 25;
const VIEW_GRID_HORIZONTAL_MINOR: usize = 26;
const VIEW_GRID_VERTICAL_MAJOR: usize = 27;
const VIEW_GRID_VERTICAL_MINOR: usize = 28;
const VIEW_GRID_GEOMETRY_POINTS: usize = 29;
/// Offsets from an axis's line component to its other components.
const AXIS_MAJOR_ANNOTATION: usize = 2;
const AXIS_MINOR_ANNOTATION: usize = 3;
const AXIS_MAJOR_TICKS: usize = 4;
const AXIS_MINOR_TICKS: usize = 5;

/// Profile style components in profile views: a leading record, then
/// `ProfileDisplayStyleProfileType` in value order.
const PROFILE_LINE: usize = 1;
const PROFILE_CURVE: usize = 2;
const PROFILE_SYMMETRICAL_PARABOLA: usize = 3;
const PROFILE_ASYMMETRICAL_PARABOLA: usize = 4;
const PROFILE_LINE_EXTENSIONS: usize = 6;
/// Data band style: ticks at the band's major stations.
const BAND_MAJOR_TICKS: usize = 3;

/// Multiples of `step` from `from` to `to`, both ends included.
fn multiples(from: f64, to: f64, step: f64) -> Vec<f64> {
    if !(step > 0.0) || to < from {
        return Vec::new();
    }
    let eps = 1e-9 * step.max(1.0);
    let first = (from / step - eps).ceil() as i64;
    let last = (to / step + eps).floor() as i64;
    (first..=last.min(first + 10_000)).map(|i| i as f64 * step).collect()
}

/// Axes, ticks, annotation, grid and profiles of a profile view, each
/// styled by its component; profiles by the style the view gives them.
fn profile_view(data: &ArchEntityData, document: &document::Document, out: &mut Decoded) -> Option<()> {
    let frame = data.profile_view_frame()?;
    let handles = data.decode().handles;
    let view_style = handles
        .iter()
        .filter_map(|h| object_arch(document, *h))
        .find(|o| o.dxf_name == "AECC_PROFILE_VIEW_STYLE");
    let components = view_style.map(|s| s.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h)));
    let style = |index| component(document, components.as_deref(), index);
    let scale = drawing_scale(document);
    let at = |station: f64, elevation: f64| {
        let p = frame.to_drawing(station, elevation);
        [p.x, p.y, 0.0]
    };
    let (s0, s1, e0, e1) = (
        frame.station_min,
        frame.station_max,
        frame.elevation_min,
        frame.elevation_max,
    );
    styled(out, style(VIEW_LEFT), vec![at(s0, e0), at(s0, e1)]);
    styled(out, style(VIEW_RIGHT), vec![at(s1, e0), at(s1, e1)]);
    styled(out, style(VIEW_TOP), vec![at(s0, e1), at(s1, e1)]);
    styled(out, style(VIEW_BOTTOM), vec![at(s0, e0), at(s1, e0)]);
    // A vertical line at every geometry point of the view's alignment.
    if let Some(alignment) = handles.iter().find_map(|h| arch_of(document, *h, CivilKind::Alignment)) {
        let start = alignment.alignment_start_station().unwrap_or(0.0);
        for point in alignment.alignment_geometry_points() {
            let station = start + point.distance;
            if (s0..=s1).contains(&station) {
                styled(out, style(VIEW_GRID_GEOMETRY_POINTS), vec![at(station, e0), at(station, e1)]);
            }
        }
    }
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
    let shown = shown_stations(alignment_among(document, &handles));
    let elevation_options = field_options(ELEVATION).unwrap_or_default();
    if let Some(axes) = view_style.and_then(|s| s.profile_view_axes()) {
        for elevation in multiples(e0, e1, axes.left.major_interval) {
            styled(out, style(VIEW_GRID_HORIZONTAL_MAJOR), vec![at(s0, elevation), at(s1, elevation)]);
        }
        for elevation in multiples(e0, e1, axes.left.minor_interval) {
            styled(out, style(VIEW_GRID_HORIZONTAL_MINOR), vec![at(s0, elevation), at(s1, elevation)]);
        }
        for station in multiples(s0, s1, axes.bottom.major_interval) {
            styled(out, style(VIEW_GRID_VERTICAL_MAJOR), vec![at(station, e0), at(station, e1)]);
        }
        for station in multiples(s0, s1, axes.bottom.minor_interval) {
            styled(out, style(VIEW_GRID_VERTICAL_MINOR), vec![at(station, e0), at(station, e1)]);
        }
        // (axis, line component, ticks run along stations, fixed value,
        // outward direction, text alignment)
        let sides = [
            (axes.left, VIEW_LEFT, false, s0, Vector2::new(-1.0, 0.0), [1.0, 0.5]),
            (axes.right, VIEW_RIGHT, false, s1, Vector2::new(1.0, 0.0), [0.0, 0.5]),
            (axes.top, VIEW_TOP, true, e1, Vector2::new(0.0, 1.0), [0.5, 0.0]),
            (axes.bottom, VIEW_BOTTOM, true, e0, Vector2::new(0.0, -1.0), [0.5, 1.0]),
        ];
        for (axis, line, along_stations, fixed, outward, align) in sides {
            let (from, to) = if along_stations { (s0, s1) } else { (e0, e1) };
            let point = |v: f64| {
                let [x, y, _] = if along_stations { at(v, fixed) } else { at(fixed, v) };
                Vector2::new(x, y)
            };
            let options = if along_stations { &station_options } else { &elevation_options };
            for (interval, size, height, ticks, annotation) in [
                (axis.minor_interval, axis.minor_tick_size, axis.minor_text_height, AXIS_MINOR_TICKS, AXIS_MINOR_ANNOTATION),
                (axis.major_interval, axis.major_tick_size, axis.major_text_height, AXIS_MAJOR_TICKS, AXIS_MAJOR_ANNOTATION),
            ] {
                let half = size * scale / 2.0;
                let mut values: Vec<(f64, bool)> = multiples(from, to, interval).into_iter().map(|v| (v, true)).collect();
                // A station axis also labels the view's first and last
                // station, without a tick of its own.
                if along_stations && annotation == AXIS_MAJOR_ANNOTATION {
                    for end in [from, to] {
                        if !values.iter().any(|(v, _)| (v - end).abs() < 1e-6) {
                            values.push((end, false));
                        }
                    }
                }
                for (value, tick) in values {
                    let p = point(value);
                    let (a, b) = (p - outward * half, p + outward * half);
                    if tick {
                        styled(out, style(line + ticks), vec![[a.x, a.y, 0.0], [b.x, b.y, 0.0]]);
                    }
                    if let Some(text_style) = style(line + annotation) {
                        out.texts.push(ProxyText {
                            position: [b.x, b.y, 0.0],
                            height: height * scale,
                            rotation: 0.0,
                            width_factor: 1.0,
                            oblique: 0.0,
                            text: LabelPart::format_value(options, if along_stations { shown(value) } else { value }),
                            font: label_font(document),
                            style: text_style,
                            align,
                        });
                    }
                }
            }
        }
    }
    // The view lists each profile, followed by the style it draws that
    // profile with; without one the profile's own style applies.
    // The view's bands list profiles again; each profile draws once.
    let mut drawn: Vec<u64> = Vec::new();
    for (index, handle) in handles.iter().enumerate() {
        let Some(profile) = profile_entity(document, *handle) else {
            continue;
        };
        if drawn.contains(handle) {
            continue;
        }
        drawn.push(*handle);
        let given = handles
            .get(index + 1)
            .and_then(|h| object_arch(document, *h))
            .filter(|o| o.dxf_name == "AECC_PROFILE_STYLE")
            .map(|o| o.display_components(|h| document.layers.iter().any(|l| l.handle.value() == h)));
        let profile_components =
            given.or_else(|| style_components(document, &profile.decode().handles, "AECC_PROFILE_STYLE"));
        for (piece, run) in profile.profile_pieces() {
            let index = match piece {
                ProfilePiece::Tangent => PROFILE_LINE,
                ProfilePiece::Circular => PROFILE_CURVE,
                ProfilePiece::Parabola => PROFILE_SYMMETRICAL_PARABOLA,
                ProfilePiece::AsymmetricParabola => PROFILE_ASYMMETRICAL_PARABOLA,
            };
            let piece_style = component(document, profile_components.as_deref(), index);
            for clipped in clip_to_view(&frame, &run) {
                styled(out, piece_style, clipped);
            }
        }
        // Tangents carried on to the PVI over each vertical curve.
        let extension_style = component(document, profile_components.as_deref(), PROFILE_LINE_EXTENSIONS);
        for w in profile.profile_pvis().windows(3) {
            let (a, pvi, b) = (w[0].point, w[1], w[2].point);
            let half = pvi.curve_length / 2.0;
            if half <= 0.0 {
                continue;
            }
            let p = pvi.point;
            let grade = |q: Vector2| (p.y - q.y) / (p.x - q.x);
            let run = [
                Vector2::new(p.x - half, p.y - grade(a) * half),
                p,
                Vector2::new(p.x + half, p.y + grade(b) * half),
            ];
            for clipped in clip_to_view(&frame, &run) {
                styled(out, extension_style, clipped);
            }
        }
    }
    Some(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_walls_stop_at_structures() {
        let v = |x: f64, y: f64| Vector2::new(x, y);
        // A wall from x = 0 to 10 along y = 0.5, through a manhole of
        // radius 1 at the start and a box structure around the end.
        let footprints = [
            Footprint::Circle(v(0.0, 0.0), 1.0),
            Footprint::Box(v(9.0, -2.0), v(11.0, 2.0)),
        ];
        let kept = outside_footprints(v(0.0, 0.5), v(10.0, 0.5), &footprints);
        assert_eq!(kept.len(), 1);
        let (a, b) = kept[0];
        assert!((a.x - 0.75f64.sqrt()).abs() < 1e-9 && (a.y - 0.5).abs() < 1e-9);
        assert!((b.x - 9.0).abs() < 1e-9);
        assert!(footprints[0].contains(v(0.2, 0.2)) && !footprints[1].contains(v(8.0, 0.0)));
    }
}
