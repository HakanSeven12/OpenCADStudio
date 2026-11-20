//! The civil commands carried out: each [`SurfaceRequest`] of a civil command written into the
//! drawing as native Civil 3D records by the codec's civil writers, as one undo step.

use document::classes::civil as c3d;
use document::types::{Vector2, Vector3};
use document::{EntityType, Handle};

use crate::app::OpenCADStudio;
use crate::modules::surface::command::{civil_class, SurfaceRequest};
use crate::scene::ChangeKind;

/// "`base` n" with n one more than the drawing's objects of class `dxf`.
fn next_name(document: &document::Document, dxf: &str, base: &str) -> String {
    let n = document.entities().filter(|e| civil_class(e) == Some(dxf)).count()
        + document
            .objects
            .values()
            .filter(|o| matches!(o, document::objects::ObjectType::Arch(a) if a.dxf_name == dxf))
            .count();
    format!("{base} {}", n + 1)
}

/// The boundary kind of a corridor surface drawn along the corridor's extents (the codec's
/// `EXTENTS`).
const CORRIDOR_EXTENTS: u32 = 0x8000_0002;

/// The Civil 3D record of `handle`.
fn arch(document: &document::Document, handle: Handle) -> Option<&document::classes::arch::ArchEntityData> {
    match document.get_entity(handle)? {
        EntityType::Extended(x) => match &x.data {
            document::entities::ExtendedEntityData::Arch(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

/// The newest profile of `alignment`.
fn newest_profile(document: &document::Document, alignment: Handle) -> Option<Handle> {
    arch(document, alignment)?
        .typed_handles()
        .iter()
        .rev()
        .map(|r| Handle::new(r.0))
        .find(|h| arch(document, *h).is_some_and(|a| a.dxf_name == "AECC_PROFILE"))
}

/// The drawing's last site.
fn last_site(document: &document::Document) -> Option<Handle> {
    document
        .entities()
        .filter_map(|e| match e {
            EntityType::Extended(x) => match &x.data {
                document::entities::ExtendedEntityData::Arch(a)
                    if a.dxf_name == "AECC_PARCEL" && a.face_name().is_some() && a.parcel_site().is_none() =>
                {
                    Some(x.common.handle)
                }
                _ => None,
            },
            _ => None,
        })
        .last()
}

/// `add` on `site`; on a new site named `site_name` when there is none or when the member
/// cannot join it (a site the codec does not write, or a member that would cross another).
fn add_on_site(
    doc: &mut document::Document,
    site: &mut Option<Handle>,
    site_name: &str,
    mut add: impl FnMut(&mut document::Document, Handle) -> Result<Handle, document::error::DxfError>,
) -> Result<Handle, document::error::DxfError> {
    if let Some(s) = *site {
        if let Ok(h) = add(doc, s) {
            return Ok(h);
        }
    }
    let s = c3d::add_site(doc, site_name, "")?;
    *site = Some(s);
    add(doc, s)
}

/// The vertices (x, y, elevation) of a line or polyline with the bulge of the segment that
/// starts at each, the first again at the end of a closed one.
fn vertices(entity: &EntityType) -> Result<Vec<(Vector3, f64)>, String> {
    let (mut points, closed): (Vec<(Vector3, f64)>, bool) = match entity {
        EntityType::Line(l) => (vec![(l.start, 0.0), (l.end, 0.0)], false),
        EntityType::LwPolyline(p) => {
            if p.normal.z < 0.0 {
                return Err(crate::t!("The polyline is not in the XY plane.").into_owned());
            }
            let at = |v: &document::entities::LwVertex| Vector3::new(v.location.x, v.location.y, p.elevation);
            (p.vertices.iter().map(|v| (at(v), v.bulge)).collect(), p.is_closed)
        }
        EntityType::Polyline2D(p) => {
            let z = p.elevation;
            (p.vertices.iter().map(|v| (Vector3::new(v.location.x, v.location.y, z), v.bulge)).collect(), p.is_closed())
        }
        EntityType::Polyline3D(p) => (p.vertices.iter().map(|v| (v.position, 0.0)).collect(), p.is_closed()),
        _ => return Err(crate::t!("Select a line or polyline.").into_owned()),
    };
    points.dedup_by(|a, b| a.0.x == b.0.x && a.0.y == b.0.y);
    if closed && points.len() > 2 {
        points.push((points[0].0, 0.0));
    } else if let Some(last) = points.last_mut() {
        // An open polyline's last bulge belongs to no segment.
        last.1 = 0.0;
    }
    if points.len() < 2 {
        return Err(crate::t!("The polyline has no length.").into_owned());
    }
    Ok(points)
}

/// A feature line's own points (without the nodes other members' crossings put on it), as
/// the codec takes them back.
fn feature_line_vertices(document: &document::Document, line: Handle) -> Option<Vec<c3d::FeatureLineVertex>> {
    let container = arch(document, line)?.feature_line_container()?;
    let document::objects::ObjectType::Arch(o) = document.objects.get(&Handle::new(container))? else { return None };
    let index = o.site_members().iter().position(|h| *h == line.value())?;
    let topology = o.topology()?;
    let mut run = topology.edges.get(index)?.clone();
    // A closed edge repeats its first point last.
    if topology.edge_flags.get(index).is_some_and(|f| f & 1 != 0) {
        run.pop();
    }
    Some(
        run.iter()
            .filter(|v| v.flags & (0x400 | 2) != 0x400 || v.is_elevation_point())
            .map(|v| c3d::FeatureLineVertex { point: Vector3::new(v.point.x, v.point.y, v.z), bulge: v.bulge, elevation_point: v.is_elevation_point() })
            .collect(),
    )
}

/// A profile through `pvis` as fixed entities: a fixed tangent between curves and a fixed
/// symmetric parabola through its start, its middle and its end at each inner PVI whose
/// curve `length` (from the grade change) is positive.
fn fixed_profile(pvis: &[Vector2], length: impl Fn(f64) -> f64) -> Result<Vec<c3d::ProfileEntity>, String> {
    let grade = |a: Vector2, b: Vector2| (b.y - a.y) / (b.x - a.x);
    let mut out = Vec::new();
    let mut at = pvis[0];
    let tangent = |out: &mut Vec<c3d::ProfileEntity>, start: Vector2, end: Vector2| {
        if end.x > start.x {
            out.push(c3d::ProfileEntity::Tangent { start, end });
        }
    };
    for k in 1..pvis.len() - 1 {
        let (p, g1, g2) = (pvis[k], grade(pvis[k - 1], pvis[k]), grade(pvis[k], pvis[k + 1]));
        let l = length((g2 - g1).abs());
        if !(l > 0.0) || g1 == g2 {
            tangent(&mut out, at, p);
            at = p;
            continue;
        }
        let (start, end) = (Vector2::new(p.x - l / 2.0, p.y - g1 * l / 2.0), Vector2::new(p.x + l / 2.0, p.y + g2 * l / 2.0));
        if start.x < at.x - 1e-9 || end.x > pvis[k + 1].x + 1e-9 {
            return Err(crate::t!("The vertical curves overlap or pass the PVIs beside them.").into_owned());
        }
        tangent(&mut out, at, start);
        // A symmetric parabola passes (g2 - g1) L / 8 off its PVI.
        out.push(c3d::ProfileEntity::ParabolaThreePoints { start, middle: Vector2::new(p.x, p.y + (g2 - g1) * l / 8.0), end });
        at = end;
    }
    tangent(&mut out, at, pvis[pvis.len() - 1]);
    Ok(out)
}

/// The curve elements of `alignment` in station order: id, start and end station, and
/// whether the curve turns left.
fn alignment_curves(a: &document::classes::arch::ArchEntityData) -> Option<Vec<(i16, f64, f64, bool)>> {
    let elements = a.alignment_elements()?;
    let pieces = a.alignment_pieces();
    let start = a.alignment_start_station().unwrap_or(0.0);
    let mut out = Vec::new();
    let (mut used, mut from) = (0, 0.0);
    let mut at = elements.iter().find(|e| e.previous == 0);
    while let Some(e) = at {
        // A spiral-curve-spiral solves into three pieces, every other element into one.
        let n = if matches!(e.kind, c3d::AlignmentElementKind::FreeSpiralCurveSpiral { .. }) { 3 } else { 1 };
        let own = pieces.get(used..used + n)?;
        used += n;
        let points: Vec<Vector2> = own.iter().flat_map(|(_, p)| p.iter().copied()).collect();
        let length: f64 = own.iter().map(|(_, p)| p.windows(2).map(|w| w[0].distance(&w[1])).sum::<f64>()).sum();
        if !matches!(e.kind, c3d::AlignmentElementKind::FixedLine { .. }) && points.len() >= 3 {
            let (d0, d1) = (points[1] - points[0], points[points.len() - 1] - points[points.len() - 2]);
            out.push((e.id, start + from, start + from + length, d0.x * d1.y - d0.y * d1.x > 0.0));
        }
        from += length;
        (out.len() <= elements.len() && used <= pieces.len()).then_some(())?;
        at = (e.next != 0).then(|| elements.iter().find(|x| x.id == e.next)).flatten();
    }
    Some(out)
}

impl OpenCADStudio {
    pub(super) fn civil(&mut self, i: usize, req: &SurfaceRequest) -> Result<String, String> {
        let name = req.command;
        let layer = self.tabs[i].active_layer.clone();
        let layer = Some(layer.as_str());
        let refused = |e: document::error::DxfError| match e {
            document::error::DxfError::Parse(message) => message,
            e => e.to_string(),
        };
        let mut changes: Vec<(Handle, ChangeKind)> = Vec::new();
        let message = match name {
            "CREATEPOINTS" => {
                if req.points.is_empty() {
                    return Ok(String::new());
                }
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let mut failed = None;
                for p in &req.points {
                    match c3d::add_cogo_point(doc, None, Vector3::new(p.x, p.y, p.z), None, None, layer) {
                        Ok(h) => changes.push((h, ChangeKind::Added)),
                        Err(e) => {
                            failed = Some(refused(e));
                            break;
                        }
                    }
                }
                match failed {
                    // Nothing written: the drawing is as it was.
                    Some(error) if changes.is_empty() => return Err(error),
                    Some(error) => format!("{} {error}", crate::tf!("{} point(s) created.", changes.len())),
                    None => crate::tf!("{} point(s) created.", changes.len()).into_owned(),
                }
            }
            "CREATEPOINTGROUP" => {
                let doc = &self.tabs[i].scene.document;
                let numbers: Vec<u32> = req.objects[0]
                    .iter()
                    .filter_map(|h| match doc.get_entity(*h)? {
                        EntityType::Extended(x) => match &x.data {
                            document::entities::ExtendedEntityData::Arch(a) => a.cogo_point_number(),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                let patterns = |key: &str| {
                    req.text(key).map(|t| t.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect::<Vec<_>>())
                };
                let (low, high) = (req.values.get("elevation_min").copied(), req.values.get("elevation_max").copied());
                // One criterion: the selected points, or a name, description or elevation rule.
                let mut criteria = Vec::new();
                if !numbers.is_empty() {
                    criteria.push(c3d::PointQuery::Numbers(numbers.clone()));
                }
                if let Some(names) = patterns("name") {
                    criteria.push(c3d::PointQuery::Names(names));
                }
                if let Some(descriptions) = patterns("description") {
                    criteria.push(c3d::PointQuery::RawDescription(descriptions));
                }
                match (low, high) {
                    (Some(low), Some(high)) => criteria.push(c3d::PointQuery::ElevationRange(low.min(high), low.max(high))),
                    (Some(low), None) => criteria.push(c3d::PointQuery::Elevation(c3d::Comparison::GreaterOrEqual, low)),
                    (None, Some(high)) => criteria.push(c3d::PointQuery::Elevation(c3d::Comparison::LessOrEqual, high)),
                    (None, None) => {}
                }
                let query = match criteria.len() {
                    0 => return Err(crate::t!("No COGO points selected.").into_owned()),
                    1 => criteria.remove(0),
                    _ => return Err(crate::t!("Use one of: selected points, a name pattern, a description pattern or an elevation range.").into_owned()),
                };
                let group = next_name(doc, "AECC_POINT_PG", "Point Group");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                match query {
                    c3d::PointQuery::Numbers(_) => {
                        c3d::add_point_group(doc, &group, None, &numbers).map_err(refused)?;
                        crate::tf!("Point group {} created with {} point(s).", group, numbers.len()).into_owned()
                    }
                    query => {
                        c3d::add_point_group_query(doc, &group, None, &query).map_err(refused)?;
                        crate::tf!("Point group {} created.", group).into_owned()
                    }
                }
            }
            "CREATEPOINTLABELSTYLE" => {
                let points = req.objects[0].clone();
                let height = req.values.get("height").copied();
                let color = req.values.get("color").copied();
                if height.is_some_and(|h| !(h > 0.0)) || color.is_some_and(|c| !(1.0..=255.0).contains(&c) || c.fract() != 0.0) {
                    return Err(crate::t!("The text height must be positive and the color 1 to 255.").into_owned());
                }
                let orientation = req.text("orientation").and_then(|t| crate::modules::civil::LABEL_ORIENTATIONS.iter().position(|o| *o == t)).map(|o| o as i32);
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                // The first free "Point Label Style n".
                let mut k = 1;
                let (style, h) = loop {
                    let style = format!("Point Label Style {k}");
                    let made = c3d::add_point_label_style(doc, &style, |s| {
                        if orientation.is_some() {
                            s.settings.orientation = orientation;
                        }
                        for c in &mut s.components {
                            if height.is_some() {
                                c.height = height;
                            }
                            if let Some(c3) = color {
                                c.text_color = Some(document::types::Color::from_index(c3 as i16));
                            }
                        }
                    });
                    match made {
                        Ok(h) => break (style, h),
                        Err(e) if k < 1000 && e.to_string().contains("exists") => k += 1,
                        Err(e) => return Err(refused(e)),
                    }
                };
                for p in &points {
                    c3d::set_cogo_point_label_style(doc, *p, h).map_err(refused)?;
                    changes.push((*p, ChangeKind::Modified));
                }
                crate::tf!("Label style {} created and given to {} point(s).", style, points.len()).into_owned()
            }
            "CREATEALIGNMENT" => {
                let doc = &self.tabs[i].scene.document;
                let entity = req.objects[0].first().and_then(|h| doc.get_entity(*h)).ok_or("no object")?;
                let points = vertices(entity)?;
                if points.iter().any(|p| p.1 != 0.0) {
                    return Err(crate::t!("Polylines with arcs cannot be made into alignments yet.").into_owned());
                }
                let points: Vec<Vector2> = points.iter().map(|p| Vector2::new(p.0.x, p.0.y)).collect();
                let radius = req.value("radius", 0.0);
                let spiral = req.value("spiral", 0.0);
                let corner = if radius <= 0.0 {
                    c3d::VertexCurve::None
                } else if spiral <= 0.0 {
                    c3d::VertexCurve::Arc(radius)
                } else {
                    c3d::VertexCurve::SpiralCurveSpiral { spiral_in: spiral, radius, spiral_out: spiral }
                };
                let kind = match req.text("type") {
                    Some("Utility") => c3d::AlignmentKind::Utility,
                    Some("Rail") => c3d::AlignmentKind::Rail { gauge: req.value("gauge", 1.435) },
                    _ => c3d::AlignmentKind::Centerline,
                };
                let alignment = next_name(doc, "AECC_ALIGNMENT", "Alignment");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let curves = vec![corner; points.len().saturating_sub(2)];
                let h = c3d::add_alignment_of_kind(doc, &alignment, &points, &curves, kind, layer).map_err(refused)?;
                changes.push((h, ChangeKind::Added));
                crate::tf!("Alignment {} created.", alignment).into_owned()
            }
            "CREATEOFFSETALIGNMENT" => {
                let parent = *req.objects[0].first().ok_or("no alignment")?;
                let offset = req.value("offset", 3.5);
                let alignment = next_name(&self.tabs[i].scene.document, "AECC_ALIGNMENT", "Alignment");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_offset_alignment(doc, parent, &alignment, offset, None, layer).map_err(refused)?;
                changes.push((parent, ChangeKind::Modified));
                changes.push((h, ChangeKind::Added));
                crate::tf!("Offset alignment {} created.", alignment).into_owned()
            }
            "ADDDESIGNSPEEDS" | "ADDSTATIONEQUATIONS" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                if req.points.is_empty() {
                    return Err(crate::t!("No stations given.").into_owned());
                }
                let label = arch(&self.tabs[i].scene.document, alignment).and_then(|a| a.decode().strings.get(3).cloned()).unwrap_or_default();
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                changes.push((alignment, ChangeKind::Modified));
                if name == "ADDDESIGNSPEEDS" {
                    let mut speeds: Vec<(f64, f64)> = req.points.iter().map(|p| (p.x, p.y)).collect();
                    speeds.sort_by(|a, b| a.0.total_cmp(&b.0));
                    c3d::add_design_speeds(doc, alignment, &speeds).map_err(refused)?;
                    crate::tf!("{} design speed(s) added to {}.", speeds.len(), label).into_owned()
                } else {
                    let increasing = req.text("direction") != Some(crate::modules::civil::EQUATION_DIRECTIONS[1]);
                    let mut equations: Vec<(f64, f64, bool)> = req.points.iter().map(|p| (p.x, p.y, increasing)).collect();
                    equations.sort_by(|a, b| a.0.total_cmp(&b.0));
                    c3d::add_station_equations(doc, alignment, &equations).map_err(refused)?;
                    crate::tf!("{} station equation(s) added to {}.", equations.len(), label).into_owned()
                }
            }
            "ADDSUPERELEVATION" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let (nc, e, lr) = (req.value("normal", 0.02), req.value("rate", 0.06), req.value("runoff", 30.0));
                if !(nc > 0.0 && e > nc && lr > 0.0) {
                    return Err(crate::t!("The superelevation needs a rate above the normal slope and a positive runoff length.").into_owned());
                }
                let a = arch(&self.tabs[i].scene.document, alignment).ok_or("no alignment")?;
                let elements = alignment_curves(a).ok_or_else(|| crate::t!("The alignment cannot be solved.").into_owned())?;
                if elements.is_empty() {
                    return Err(crate::t!("The alignment has no curves.").into_owned());
                }
                // Each curve: from level crown through reverse crown to full superelevation a
                // third of the runoff into the curve, and back the same way at its end. Lane
                // slopes run away from the crown; the outside of the curve rises.
                let curves: Vec<c3d::SuperelevationCurve> = elements
                    .iter()
                    .map(|&(element, s0, s1, left)| {
                        let mid = (s0 + s1) / 2.0;
                        let (begin, end) = ((s0 + lr / 3.0).min(mid), (s1 - lr / 3.0).max(mid));
                        let side = |outside: f64, inside: f64| {
                            let (l, r) = if left { (inside, outside) } else { (outside, inside) };
                            [l, l, r, r, l, l, r, r]
                        };
                        let station = |kind, station, ending, slopes| c3d::CriticalStation { kind, station, ending, slopes };
                        use c3d::CriticalStationKind as K;
                        c3d::SuperelevationCurve {
                            element,
                            stations: vec![
                                station(K::LevelCrown, begin - lr, false, side(0.0, -nc)),
                                station(K::ReverseCrown, begin - lr + lr * nc / e, false, side(nc, -nc)),
                                station(K::BeginFullSuper, begin, false, side(e, -e)),
                                station(K::EndFullSuper, end, true, side(e, -e)),
                                station(K::ReverseCrown, end + lr - lr * nc / e, true, side(nc, -nc)),
                                station(K::LevelCrown, end + lr, true, side(0.0, -nc)),
                            ],
                        }
                    })
                    .collect();
                self.push_undo_snapshot(i, name);
                c3d::add_superelevation(&mut self.tabs[i].scene.document, alignment, &curves).map_err(refused)?;
                changes.push((alignment, ChangeKind::Modified));
                crate::tf!("Superelevation added to {} curve(s).", curves.len()).into_owned()
            }
            "CREATESURFACE" => {
                let doc = &self.tabs[i].scene.document;
                let cogo = c3d::cogo_points(doc);
                let mut points: Vec<Vector3> = Vec::new();
                for h in &req.objects[0] {
                    let p = match doc.get_entity(*h) {
                        Some(EntityType::Point(p)) => Some(p.location),
                        Some(EntityType::Extended(x)) => match &x.data {
                            document::entities::ExtendedEntityData::Arch(a) => a
                                .cogo_point_number()
                                .and_then(|n| cogo.iter().find(|c| c.number == n))
                                .map(|c| c.position),
                            _ => None,
                        },
                        _ => None,
                    };
                    // A surface takes one point per plan position.
                    if let Some(p) = p.filter(|p| !points.iter().any(|q| q.x == p.x && q.y == p.y)) {
                        points.push(p);
                    }
                }
                if points.len() < 3 {
                    return Err(crate::t!("A surface needs three or more points apart in plan.").into_owned());
                }
                let count = points.len();
                // The points, then the selected breaklines and outer boundary, each a definition
                // whose lines come from the selected objects.
                let definition = |operation, texts: &[&str], sources: &[Handle]| c3d::TinDefinition {
                    operation,
                    id: [0; 16],
                    texts: texts.iter().map(|t| t.to_string()).collect(),
                    sources: sources.iter().map(|h| h.value()).collect(),
                };
                let lines = |sources: &[Handle]| c3d::TinLines {
                    objects: sources.len() as u32,
                    polylines: Vec::new(),
                    mid_ordinate: 1.0,
                    maximum_distance: 0.0,
                    weeding_distance: 0.0,
                    weeding_angle: 0.0,
                };
                let mut definitions = vec![definition(c3d::TinOperation::AddPoints(points), &[], &[])];
                if let breaklines @ [_, ..] = req.objects[1].as_slice() {
                    let operation = c3d::TinOperation::Breaklines(c3d::TinBreaklineType::Standard, lines(breaklines));
                    definitions.push(definition(operation, &["Breaklines"], breaklines));
                }
                if let boundary @ [_, ..] = req.objects[2].as_slice() {
                    let operation = c3d::TinOperation::Boundary { boundary_type: c3d::TinBoundaryType::Outer, non_destructive: false, lines: lines(boundary) };
                    definitions.push(definition(operation, &["Boundary"], boundary));
                }
                let surface = next_name(doc, "AECC_TIN_SURFACE", "Surface");
                self.push_undo_snapshot(i, name);
                let h = c3d::add_tin_surface_with(&mut self.tabs[i].scene.document, &surface, &definitions, false, layer).map_err(refused)?;
                changes.push((h, ChangeKind::Added));
                crate::tf!("Surface {} created from {} point(s).", surface, count).into_owned()
            }
            "CREATEPROFILE" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let pvis: Vec<Vector2> = req.points.iter().map(|p| Vector2::new(p.x, p.y)).collect();
                if pvis.len() < 2 {
                    return Err(crate::t!("A profile needs two or more PVIs.").into_owned());
                }
                let value = req.value("curve", 0.0);
                let curve = match req.text("curve_type") {
                    _ if value == 0.0 => c3d::VerticalCurve::None,
                    Some("K value") => c3d::VerticalCurve::ParabolaK(value),
                    Some("Radius") => c3d::VerticalCurve::ParabolaRadius(value),
                    _ => c3d::VerticalCurve::Parabola(value),
                };
                let curves = vec![curve; pvis.len() - 2];
                // Fixed entities: each curve's length from its grade change.
                let fixed = match req.text("layout") == Some(crate::modules::civil::PROFILE_LAYOUTS[1]) {
                    false => None,
                    true => Some(fixed_profile(&pvis, |change| match curve {
                        c3d::VerticalCurve::ParabolaK(k) => k * change * 100.0,
                        c3d::VerticalCurve::ParabolaRadius(r) => r * change,
                        c3d::VerticalCurve::Parabola(l) => l,
                        _ => 0.0,
                    })?),
                };
                let profile = next_name(&self.tabs[i].scene.document, "AECC_PROFILE", "Profile");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = match &fixed {
                    Some(entities) => c3d::add_profile_entities(doc, alignment, &profile, entities, layer),
                    None => c3d::add_profile_with_curves(doc, alignment, &profile, &pvis, &curves, layer),
                }
                .map_err(refused)?;
                changes.push((alignment, ChangeKind::Modified));
                changes.push((h, ChangeKind::Added));
                crate::tf!("Profile {} created.", profile).into_owned()
            }
            "CREATESURFACEPROFILE" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let surface = *req.objects[1].first().ok_or_else(|| crate::t!("Select a TIN surface.").into_owned())?;
                let offset = req.value("offset", 0.0);
                let profile = next_name(&self.tabs[i].scene.document, "AECC_PROFILE", "Profile");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_surface_profile(doc, alignment, surface, &profile, offset, None, layer).map_err(refused)?;
                changes.push((alignment, ChangeKind::Modified));
                changes.push((h, ChangeKind::Added));
                crate::tf!("Surface profile {} created.", profile).into_owned()
            }
            "CREATEOFFSETPROFILE" => {
                let doc = &self.tabs[i].scene.document;
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let parent = arch(doc, alignment)
                    .and_then(|a| a.alignment_offset_parent())
                    .map(Handle::new)
                    .ok_or_else(|| crate::t!("Select an offset alignment.").into_owned())?;
                // The parent's newest profile, as a corridor takes its baseline's.
                let source = newest_profile(doc, parent).ok_or_else(|| crate::t!("The offset alignment's parent has no profile.").into_owned())?;
                let profile = next_name(doc, "AECC_PROFILE", "Profile");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_offset_profile(doc, source, alignment, &profile, req.value("slope", 0.0), layer).map_err(refused)?;
                changes.extend([(parent, ChangeKind::Modified), (alignment, ChangeKind::Modified), (source, ChangeKind::Modified), (h, ChangeKind::Added)]);
                crate::tf!("Offset profile {} created.", profile).into_owned()
            }
            "CREATEPROFILEVIEW" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let corner = req.points.first().ok_or("no corner")?;
                let view = next_name(&self.tabs[i].scene.document, "AECC_PROFILE_VIEW", "Profile View");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                // A range given at the closing prompt, both of its ends; none: the automatic one.
                let range = |lo: &str, hi: &str| -> Result<Option<(f64, f64)>, String> {
                    match (req.values.get(lo), req.values.get(hi)) {
                        (None, None) => Ok(None),
                        (Some(a), Some(b)) => Ok(Some((*a, *b))),
                        _ => Err(crate::t!("Give both ends of a station or elevation range.").into_owned()),
                    }
                };
                let stations = range("station_start", "station_end")?;
                let elevations = range("elevation_min", "elevation_max")?;
                // Another vertical exaggeration: the drawing's view style with it, or a new one.
                let style = match req.values.get("exaggeration").copied() {
                    None => None,
                    Some(v) => {
                        let found = doc.objects.values().find_map(|o| match o {
                            document::objects::ObjectType::Arch(a) if a.profile_view_style_settings().is_some_and(|s| s.vertical_exaggeration == v) => Some(a.handle),
                            _ => None,
                        });
                        match found {
                            Some(h) => Some(h),
                            None => Some(c3d::add_profile_view_style(doc, &format!("Exaggeration {v}"), v).map_err(refused)?),
                        }
                    }
                };
                c3d::add_profile_view_with_style(doc, alignment, &view, Vector2::new(corner.x, corner.y), style, stations, elevations, layer)
                    .map_err(refused)?;
                // The view brings its label groups: redraw everything.
                crate::tf!("Profile view {} created.", view).into_owned()
            }
            "CREATEFEATURELINE" => {
                let doc = &self.tabs[i].scene.document;
                let paths = req.objects[0]
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .map(|e| {
                        let points = vertices(e)?;
                        Ok(points
                            .into_iter()
                            .map(|(point, bulge)| c3d::FeatureLineVertex { point, bulge, elevation_point: false })
                            .collect::<Vec<_>>())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut site = last_site(doc);
                let first = next_name(doc, "AECC_FEATURE_LINE", "Feature Line");
                let site_name = next_name(doc, "AECC_PARCEL", "Site");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let base = first.rsplit_once(' ').and_then(|(_, n)| n.parse::<usize>().ok()).unwrap_or(1);
                for (k, points) in paths.iter().enumerate() {
                    let line = format!("Feature Line {}", base + k);
                    let h = add_on_site(doc, &mut site, &site_name, |doc, s| {
                        c3d::add_feature_line_vertices(doc, s, &line, points, layer)
                    })
                    .map_err(refused)?;
                    changes.push((h, ChangeKind::Added));
                }
                crate::tf!("{} feature line(s) created.", changes.len()).into_owned()
            }
            "EDITFEATURELINE" => {
                let line = *req.objects[0].first().ok_or("no feature line")?;
                let mut vertices = feature_line_vertices(&self.tabs[i].scene.document, line).ok_or_else(|| crate::t!("The feature line cannot be read.").into_owned())?;
                if req.points.is_empty() {
                    return Err(crate::t!("No points given.").into_owned());
                }
                let delete = req.text("mode") == Some(crate::modules::civil::FEATURE_LINE_EDITS[1]);
                let apart = |v: &c3d::FeatureLineVertex, p: Vector3| (v.point.x - p.x).hypot(v.point.y - p.y);
                for p in &req.points {
                    let p = Vector3::new(p.x, p.y, p.z);
                    let nearest = (0..vertices.len()).min_by(|a, b| apart(&vertices[*a], p).total_cmp(&apart(&vertices[*b], p))).ok_or("no points")?;
                    if delete {
                        if vertices.len() <= 2 {
                            return Err(crate::t!("A feature line keeps at least two points.").into_owned());
                        }
                        vertices.remove(nearest);
                        continue;
                    }
                    // A point on one of the line's points sets its elevation; one beside a
                    // straight segment becomes an elevation point there.
                    if apart(&vertices[nearest], p) <= 1e-9 * (1.0 + p.x.abs().max(p.y.abs())) {
                        vertices[nearest].point.z = p.z;
                        continue;
                    }
                    let (k, t) = vertices
                        .windows(2)
                        .enumerate()
                        .filter(|(_, w)| w[0].bulge == 0.0)
                        .filter_map(|(k, w)| {
                            let (a, b) = (w[0].point, w[1].point);
                            let (dx, dy) = (b.x - a.x, b.y - a.y);
                            let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy);
                            let off = (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy);
                            (t > 0.0 && t < 1.0).then_some((k, t, off))
                        })
                        .min_by(|a, b| a.2.total_cmp(&b.2))
                        .map(|(k, t, _)| (k, t))
                        .ok_or_else(|| crate::t!("The point is not beside a straight segment of the feature line.").into_owned())?;
                    let (a, b) = (vertices[k].point, vertices[k + 1].point);
                    let at = Vector3::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y), p.z);
                    vertices.insert(k + 1, c3d::FeatureLineVertex { point: at, bulge: 0.0, elevation_point: true });
                }
                // The ends stay PIs.
                let n = vertices.len();
                vertices[0].elevation_point = false;
                vertices[n - 1].elevation_point = false;
                self.push_undo_snapshot(i, name);
                c3d::set_feature_line_vertices(&mut self.tabs[i].scene.document, line, &vertices).map_err(refused)?;
                // The site's container is written again: redraw everything.
                crate::t!("Feature line edited.").into_owned()
            }
            "CREATEPARCEL" => {
                let doc = &self.tabs[i].scene.document;
                // Each line or polyline: its vertices with their bulges, and whether it closes.
                let lines = req.objects[0]
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .map(|e| {
                        let mut points = vertices(e)?;
                        let closed = points.len() > 3 && points.first().map(|p| (p.0.x, p.0.y)) == points.last().map(|p| (p.0.x, p.0.y));
                        if closed {
                            // A closed lot line closes back to its first vertex itself, with the
                            // last vertex's bulge.
                            points.pop();
                        }
                        Ok((points.iter().map(|p| (Vector2::new(p.0.x, p.0.y), p.1)).collect::<Vec<_>>(), closed))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut site = last_site(doc);
                let site_name = next_name(doc, "AECC_PARCEL", "Site");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let mut parcels = 0;
                for (points, closed) in &lines {
                    let mut made = Vec::new();
                    add_on_site(doc, &mut site, &site_name, |doc, s| {
                        let (line, new) = c3d::add_lot_line(doc, s, points, *closed, layer)?;
                        made = new;
                        Ok(line)
                    })
                    .map_err(refused)?;
                    parcels += made.len();
                }
                // A crossing line changes earlier parcels too: redraw everything.
                crate::tf!("{} parcel(s) created.", parcels).into_owned()
            }
            "CREATEASSEMBLY" => {
                let at = *req.points.first().ok_or("no location")?;
                let pkgs = super::civil_packages::road();
                let sources = pkgs.iter().map(super::civil_packages::install).collect::<Result<Vec<_>, _>>()?;
                // The values set at the closing prompt; the package's defaults for the rest.
                let values: Vec<(String, c3d::pkt::PktValue)> = crate::modules::civil::ASSEMBLY_KEYS
                    .iter()
                    .filter_map(|k| req.values.get(k).map(|v| (k.to_string(), c3d::pkt::PktValue::Double(*v))))
                    .collect();
                let assembly = next_name(&self.tabs[i].scene.document, "AECC_ASSEMBLY", "Assembly");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_assembly(doc, &assembly, Vector3::new(at.x, at.y, at.z), layer).map_err(refused)?;
                // Each side: the lane at the baseline, the shoulder on its outer point, the
                // side slope on the shoulder's.
                for (side, label) in [(c3d::pkt::PktSide::Right, "Right"), (c3d::pkt::PktSide::Left, "Left")] {
                    let mut parent = None;
                    for ((pkg, source), part) in pkgs.iter().zip(&sources).zip(["Lane", "Shoulder", "Side Slope"]) {
                        let name = format!("{part} {label}");
                        let made = match parent {
                            None => c3d::add_pkt_subassembly(doc, h, &name, source, pkg, side, &values, layer),
                            Some(p) => c3d::add_pkt_subassembly_on(doc, p, super::civil_packages::OUTER, &name, source, pkg, side, &values, layer),
                        };
                        parent = Some(made.map_err(refused)?);
                    }
                }
                crate::tf!("Assembly {} created.", assembly).into_owned()
            }
            "CREATECORRIDOR" => {
                let doc = &self.tabs[i].scene.document;
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let assembly = *req.objects[1].first().ok_or("no assembly")?;
                let profile = newest_profile(doc, alignment).ok_or_else(|| crate::t!("The alignment has no profile.").into_owned())?;
                let corridor = next_name(doc, "AECC_CORRIDOR", "Corridor");
                let al = arch(doc, alignment).ok_or("no alignment")?;
                let start = al.alignment_start_station().ok_or("the alignment has no start station")?;
                let length = al.alignment_geometry_points().last().map(|g| g.distance).ok_or("the alignment has no length")?;
                // Each surface follows one link code, corrected to the top or the bottom links,
                // and is bounded by the corridor's extents.
                let codes: &[(&str, i32)] = match req.text("surfaces").unwrap_or(crate::modules::civil::CORRIDOR_SURFACES[0]) {
                    "Top" => &[("Top", 1)],
                    "Datum" => &[("Datum", 2)],
                    "Top and Datum" => &[("Top", 1), ("Datum", 2)],
                    _ => &[],
                };
                let surfaces: Vec<c3d::CorridorSurfaceDefinition> = codes
                    .iter()
                    .map(|(code, correction)| c3d::CorridorSurfaceDefinition {
                        name: format!("{corridor} - {code}"),
                        build: true,
                        link_codes: vec![(code.to_string(), true)],
                        feature_line_codes: Vec::new(),
                        overhang_correction: *correction,
                    })
                    .collect();
                let boundaries = surfaces
                    .iter()
                    .map(|s| c3d::CorridorBoundary {
                        surface: s.name.clone(),
                        name: "Corridor Extents".into(),
                        description: String::new(),
                        kind: CORRIDOR_EXTENTS,
                        corners: Vec::new(),
                        components: 0,
                    })
                    .collect();
                let region = c3d::CorridorRegion {
                    name: "Region (1)".into(),
                    assembly: assembly.value(),
                    start,
                    end: start + length,
                    frequency: c3d::RegionFrequency::default(),
                    targets: Vec::new(),
                };
                let baseline = c3d::CorridorBaseline {
                    name: "Baseline (1)".into(),
                    feature_line: 0,
                    alignment: alignment.value(),
                    profile: profile.value(),
                    regions: vec![region],
                    stations: Vec::new(),
                    built: false,
                };
                let geometry_lock = req.text("lock") == Some(crate::modules::civil::CORRIDOR_LOCKS[1]);
                let def = c3d::CorridorDefinition {
                    name: corridor.clone(),
                    surfaces,
                    boundaries,
                    baselines: vec![baseline],
                    geometry_lock,
                    ..c3d::CorridorDefinition::default()
                };
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_corridor_with(doc, &def, layer).map_err(refused)?;
                // A geometry lock writes the alignment again with an anchor at each region end.
                changes.push((alignment, ChangeKind::Modified));
                changes.push((h, ChangeKind::Added));
                crate::tf!("Corridor {} created; it is built when the corridor is rebuilt.", corridor).into_owned()
            }
            "CREATESAMPLELINES" => {
                let doc = &self.tabs[i].scene.document;
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let a = arch(doc, alignment).ok_or("no alignment")?;
                let increment = req.value("increment", 20.0);
                let swath = req.value("swath", 20.0);
                if increment <= 0.0 || swath <= 0.0 {
                    return Err(crate::t!("The increment and the swath width must be positive.").into_owned());
                }
                let start = a.alignment_start_station().unwrap_or(0.0);
                let length: f64 = a
                    .alignment_pieces()
                    .iter()
                    .map(|(_, p)| p.windows(2).map(|w| w[0].distance(&w[1])).sum::<f64>())
                    .sum();
                let mut stations: Vec<f64> = (0..).map(|k| start + k as f64 * increment).take_while(|s| *s < start + length).collect();
                stations.push(start + length);
                let surfaces = req.objects[1].clone();
                let group = next_name(doc, "AECC_SAMPLE_LINE_GROUP", "SL Collection");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let g = c3d::add_sample_line_group(doc, alignment, &group, layer).map_err(refused)?;
                changes.push((alignment, ChangeKind::Modified));
                changes.push((g, ChangeKind::Added));
                for (k, station) in stations.iter().enumerate() {
                    let h = c3d::add_sample_line(doc, g, &format!("SL-{}", k + 1), *station, swath, swath, layer).map_err(refused)?;
                    changes.push((h, ChangeKind::Added));
                }
                for s in &surfaces {
                    for h in c3d::add_sections(doc, g, *s).map_err(refused)? {
                        changes.push((h, ChangeKind::Added));
                    }
                }
                crate::tf!("Sample line group {} created with {} line(s).", group, stations.len()).into_owned()
            }
            "CREATESECTIONVIEWS" => {
                let doc = &self.tabs[i].scene.document;
                let group = *req.objects[0].first().ok_or("no sample line group")?;
                let at = *req.points.first().ok_or("no point")?;
                let group_name = arch(doc, group).and_then(|g| g.decode().strings.get(2).cloned()).unwrap_or_default();
                let lines: Vec<Handle> = arch(doc, group)
                    .ok_or("no sample line group")?
                    .typed_handles()
                    .iter()
                    .map(|r| Handle::new(r.0))
                    .filter(|h| arch(doc, *h).is_some_and(|a| a.dxf_name == "AECC_SAMPLE_LINE"))
                    .collect();
                if lines.is_empty() {
                    return Err(crate::t!("The group has no sample lines.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                // The group's own group plot when the codec writes one (lines with at most one
                // section, not sampled yet); otherwise a view per line.
                if let Ok(views) = c3d::add_section_view_group(doc, group, Vector2::new(at.x, at.y), layer) {
                    changes.push((group, ChangeKind::Modified));
                    changes.extend(lines.iter().map(|l| (*l, ChangeKind::Modified)));
                    changes.extend(views.iter().map(|v| (*v, ChangeKind::Added)));
                    let message = crate::tf!("{} section view(s) created.", views.len()).into_owned();
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.bump_entities(&changes);
                    return Ok(message);
                }
                // Five views a row, spaced by the first view's frame.
                const COLUMNS: usize = 5;
                const GAP: f64 = 20.0;
                let mut step = None;
                let mut failed = None;
                for (k, line) in lines.iter().enumerate() {
                    let (dx, dy) = step.unwrap_or((0.0, 0.0));
                    let origin = Vector2::new(at.x + (k % COLUMNS) as f64 * dx, at.y - (k / COLUMNS) as f64 * dy);
                    let view = format!("{group_name} - {}", k + 1);
                    match c3d::add_section_view(doc, *line, &view, origin, layer) {
                        Ok(h) => {
                            changes.push((*line, ChangeKind::Modified));
                            changes.push((h, ChangeKind::Added));
                            if step.is_none() {
                                let f = arch(doc, h).and_then(|a| a.section_view_frame()).ok_or("no section view frame")?;
                                let height = (f.elevation_max - f.elevation_min) * f.vertical_scale;
                                step = Some((f.offset_max - f.offset_min + GAP, height + GAP));
                            }
                        }
                        Err(e) => {
                            failed = Some(refused(e));
                            break;
                        }
                    }
                }
                let count = changes.len() / 2;
                match failed {
                    Some(error) if count == 0 => return Err(error),
                    Some(error) => format!("{} {error}", crate::tf!("{} section view(s) created.", count)),
                    None => crate::tf!("{} section view(s) created.", count).into_owned(),
                }
            }
            "CREATEINTERSECTION" => {
                let doc = &self.tabs[i].scene.document;
                let main = *req.objects[0].first().ok_or("no alignment")?;
                // The second step's selection keeps the main road picked before it.
                let other = *req.objects[1].iter().find(|h| **h != main).ok_or("no alignment")?;
                let profiles = newest_profile(doc, main).zip(newest_profile(doc, other));
                let intersection = next_name(doc, "AECC_INTERSECTION", "Intersection");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                // A crossing takes the roads' profiles; a tee junction (one road ending on the
                // other) is written without them.
                let add = |doc: &mut document::Document, (a, b): (Handle, Handle)| c3d::add_intersection(doc, &intersection, main, a, other, b, ["", ""], layer);
                let h = match profiles.map(|p| add(doc, p)) {
                    Some(Ok(h)) => h,
                    _ => add(doc, (Handle::NULL, Handle::NULL)).map_err(refused)?,
                };
                changes.push((h, ChangeKind::Added));
                crate::tf!("Intersection {} created.", intersection).into_owned()
            }
            "CREATEPIPENETWORK" => {
                if req.points.is_empty() {
                    return Ok(String::new());
                }
                let structure_family = req.text("structure").map(str::to_owned);
                let pipe_family = req.text("pipe").map(str::to_owned);
                let network = next_name(&self.tabs[i].scene.document, "AECC_NETWORK", "Network");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_pipe_network(doc, &network, layer).map_err(refused)?;
                // A structure at each point, a pipe between each two, connected at its ends.
                let mut structures = Vec::new();
                for p in &req.points {
                    let at = Vector3::new(p.x, p.y, p.z);
                    let s = match &structure_family {
                        Some(family) => c3d::add_structure_of_family(doc, h, family, at, 0.0),
                        None => c3d::add_structure(doc, h, at, 0.0),
                    };
                    structures.push(s.map_err(refused)?);
                }
                for (k, w) in req.points.windows(2).enumerate() {
                    let (a, b) = (Vector3::new(w[0].x, w[0].y, w[0].z), Vector3::new(w[1].x, w[1].y, w[1].z));
                    let pipe = match &pipe_family {
                        Some(family) => c3d::add_pipe_of_family(doc, h, family, a, b),
                        None => c3d::add_pipe(doc, h, a, b),
                    }
                    .map_err(refused)?;
                    c3d::connect_pipe(doc, pipe, Some(structures[k]), Some(structures[k + 1])).map_err(refused)?;
                }
                crate::tf!("Pipe network {} created with {} structure(s).", network, structures.len()).into_owned()
            }
            "CREATEPRESSURENETWORK" => {
                if req.points.len() < 2 {
                    return Err(crate::t!("A pressure network needs at least two points.").into_owned());
                }
                let size = req.text("pipe").map(str::to_owned);
                let network = next_name(&self.tabs[i].scene.document, "AECC_PRESSUREPIPES_NETWORK", "Pressure Network");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_pressure_network(doc, &network, layer).map_err(refused)?;
                // A pipe between each two points, joined end to start.
                let mut previous = None;
                for w in req.points.windows(2) {
                    let (a, b) = (Vector3::new(w[0].x, w[0].y, w[0].z), Vector3::new(w[1].x, w[1].y, w[1].z));
                    let pipe = match &size {
                        Some(size) => c3d::add_pressure_pipe_of_size(doc, h, size, a, b, 0.0),
                        None => c3d::add_pressure_pipe(doc, h, a, b),
                    }
                    .map_err(refused)?;
                    if let Some(prev) = previous {
                        c3d::connect_pressure_parts(doc, prev, 1, pipe, 0).map_err(refused)?;
                    }
                    previous = Some(pipe);
                }
                crate::tf!("Pressure network {} created with {} pipe(s).", network, req.points.len() - 1).into_owned()
            }
            _ => return Err(crate::t!("Unknown civil command.").into_owned()),
        };
        self.tabs[i].dirty = true;
        if changes.is_empty() {
            self.tabs[i].scene.bump_geometry();
        } else {
            self.tabs[i].scene.bump_entities(&changes);
        }
        Ok(message)
    }

    /// The Name row of a selected COGO point: the typed name, none when cleared.
    pub(super) fn commit_cogo_point_name(&mut self) -> iced::Task<crate::app::Message> {
        let i = self.active_tab;
        let field = crate::entities::civil::COGO_POINT_NAME;
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i].properties.edit_buf.remove(&crate::ui::properties::FieldKey::Geom(field)) else {
            return iced::Task::none();
        };
        let name = Some(typed.trim()).filter(|t| !t.is_empty());
        let handles = self.property_target_handles(i);
        self.push_undo_snapshot(i, "PROPERTIES");
        for h in handles {
            match c3d::set_cogo_point_name(&mut self.tabs[i].scene.document, h, name) {
                Err(document::error::DxfError::Parse(message)) => self.command_line.push_error(&message),
                Err(e) => self.command_line.push_error(&e.to_string()),
                Ok(()) => {}
            }
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.bump_geometry();
        self.refresh_properties();
        iced::Task::none()
    }
}
