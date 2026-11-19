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
    add: impl Fn(&mut document::Document, Handle) -> Result<Handle, document::error::DxfError>,
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
                if numbers.is_empty() {
                    return Err(crate::t!("No COGO points selected.").into_owned());
                }
                let group = next_name(doc, "AECC_POINT_PG", "Point Group");
                self.push_undo_snapshot(i, name);
                c3d::add_point_group(&mut self.tabs[i].scene.document, &group, None, &numbers).map_err(refused)?;
                crate::tf!("Point group {} created with {} point(s).", group, numbers.len()).into_owned()
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
                let alignment = next_name(doc, "AECC_ALIGNMENT", "Alignment");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let curves = vec![corner; points.len().saturating_sub(2)];
                let h = c3d::add_alignment_with_curves(doc, &alignment, &points, &curves, layer).map_err(refused)?;
                changes.push((h, ChangeKind::Added));
                crate::tf!("Alignment {} created.", alignment).into_owned()
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
                let surface = next_name(doc, "AECC_TIN_SURFACE", "Surface");
                self.push_undo_snapshot(i, name);
                let h = c3d::add_tin_surface(&mut self.tabs[i].scene.document, &surface, &points, layer).map_err(refused)?;
                changes.push((h, ChangeKind::Added));
                crate::tf!("Surface {} created from {} point(s).", surface, points.len()).into_owned()
            }
            "CREATEPROFILE" => {
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let pvis: Vec<Vector2> = req.points.iter().map(|p| Vector2::new(p.x, p.y)).collect();
                if pvis.len() < 2 {
                    return Err(crate::t!("A profile needs two or more PVIs.").into_owned());
                }
                let curves = vec![req.value("curve", 0.0); pvis.len() - 2];
                let profile = next_name(&self.tabs[i].scene.document, "AECC_PROFILE", "Profile");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_profile(doc, alignment, &profile, &pvis, &curves, layer).map_err(refused)?;
                changes.push((alignment, ChangeKind::Modified));
                changes.push((h, ChangeKind::Added));
                crate::tf!("Profile {} created.", profile).into_owned()
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
                c3d::add_profile_view_with_range(doc, alignment, &view, Vector2::new(corner.x, corner.y), stations, elevations, layer)
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
            "CREATEPARCEL" => {
                let doc = &self.tabs[i].scene.document;
                let boundaries = req.objects[0]
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .map(|e| {
                        let mut points = vertices(e)?;
                        let closed = points.len() > 3 && points.first().map(|p| (p.0.x, p.0.y)) == points.last().map(|p| (p.0.x, p.0.y));
                        if !closed {
                            return Err(crate::t!("A parcel needs a closed polyline.").into_owned());
                        }
                        if points.iter().any(|p| p.1 != 0.0) {
                            return Err(crate::t!("Polylines with arcs cannot be made into parcels yet.").into_owned());
                        }
                        // The lot line closes back to its first vertex itself.
                        points.pop();
                        Ok(points.iter().map(|p| Vector2::new(p.0.x, p.0.y)).collect::<Vec<_>>())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut site = last_site(doc);
                let site_name = next_name(doc, "AECC_PARCEL", "Site");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                for boundary in &boundaries {
                    let h = add_on_site(doc, &mut site, &site_name, |doc, s| c3d::add_parcel(doc, s, boundary, layer)).map_err(refused)?;
                    changes.push((h, ChangeKind::Added));
                }
                // The lot lines are new entities too: redraw everything.
                changes.clear();
                crate::tf!("{} parcel(s) created.", boundaries.len()).into_owned()
            }
            "CREATECORRIDOR" => {
                let doc = &self.tabs[i].scene.document;
                let alignment = *req.objects[0].first().ok_or("no alignment")?;
                let assembly = *req.objects[1].first().ok_or("no assembly")?;
                let profile = newest_profile(doc, alignment).ok_or_else(|| crate::t!("The alignment has no profile.").into_owned())?;
                let corridor = next_name(doc, "AECC_CORRIDOR", "Corridor");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_corridor(doc, &corridor, "Baseline (1)", "Region (1)", alignment, profile, assembly, layer)
                    .map_err(refused)?;
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
                let other = *req.objects[1].first().ok_or("no alignment")?;
                let no_profile = || crate::t!("The alignment has no profile.").into_owned();
                let main_profile = newest_profile(doc, main).ok_or_else(no_profile)?;
                let other_profile = newest_profile(doc, other).ok_or_else(no_profile)?;
                let intersection = next_name(doc, "AECC_INTERSECTION", "Intersection");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let h = c3d::add_intersection(doc, &intersection, main, main_profile, other, other_profile, ["", ""], layer)
                    .map_err(refused)?;
                changes.push((h, ChangeKind::Added));
                crate::tf!("Intersection {} created.", intersection).into_owned()
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
}
