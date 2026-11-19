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

/// The vertices (x, y, elevation) of a line or polyline, the first again at the end of a
/// closed one.
fn vertices(entity: &EntityType) -> Result<Vec<Vector3>, String> {
    let arcs = || crate::t!("Polylines with arcs cannot be made into alignments yet.").into_owned();
    let (mut points, closed) = match entity {
        EntityType::Line(l) => (vec![l.start, l.end], false),
        EntityType::LwPolyline(p) => {
            if p.vertices.iter().any(|v| v.bulge != 0.0) {
                return Err(arcs());
            }
            if p.normal.z < 0.0 {
                return Err(crate::t!("The polyline is not in the XY plane.").into_owned());
            }
            (p.vertices.iter().map(|v| Vector3::new(v.location.x, v.location.y, p.elevation)).collect(), p.is_closed)
        }
        EntityType::Polyline2D(p) => {
            if p.vertices.iter().any(|v| v.bulge != 0.0) {
                return Err(arcs());
            }
            let z = p.elevation;
            (p.vertices.iter().map(|v| Vector3::new(v.location.x, v.location.y, z)).collect(), p.is_closed())
        }
        EntityType::Polyline3D(p) => (p.vertices.iter().map(|v| v.position).collect(), p.is_closed()),
        _ => return Err(crate::t!("Select a line or polyline.").into_owned()),
    };
    points.dedup_by(|a, b| a.x == b.x && a.y == b.y);
    if closed && points.len() > 2 {
        points.push(points[0]);
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
                let points: Vec<Vector2> = vertices(entity)?.iter().map(|p| Vector2::new(p.x, p.y)).collect();
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
                c3d::add_profile_view(doc, alignment, &view, Vector2::new(corner.x, corner.y), layer).map_err(refused)?;
                // The view brings its label groups: redraw everything.
                crate::tf!("Profile view {} created.", view).into_owned()
            }
            "CREATEFEATURELINE" => {
                let doc = &self.tabs[i].scene.document;
                let paths = req.objects[0]
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .map(vertices)
                    .collect::<Result<Vec<_>, String>>()?;
                // The last site this drawing has; a new one when it has none or when a feature
                // line cannot join it (a site the codec does not write).
                let site = doc
                    .entities()
                    .filter(|e| match e {
                        EntityType::Extended(x) => matches!(&x.data,
                            document::entities::ExtendedEntityData::Arch(a)
                                if a.dxf_name == "AECC_PARCEL" && a.face_name().is_some() && a.parcel_site().is_none()),
                        _ => false,
                    })
                    .last()
                    .map(|e| e.common().handle);
                let first = next_name(doc, "AECC_FEATURE_LINE", "Feature Line");
                let site_name = next_name(doc, "AECC_PARCEL", "Site");
                self.push_undo_snapshot(i, name);
                let doc = &mut self.tabs[i].scene.document;
                let mut site = site;
                let base = first.rsplit_once(' ').and_then(|(_, n)| n.parse::<usize>().ok()).unwrap_or(1);
                for (k, points) in paths.iter().enumerate() {
                    let line = format!("Feature Line {}", base + k);
                    let added = match site {
                        Some(s) => c3d::add_feature_line(doc, s, &line, points, layer),
                        None => Err(document::error::DxfError::Parse(String::new())),
                    };
                    let h = match added {
                        Ok(h) => h,
                        Err(_) => {
                            let s = c3d::add_site(doc, &site_name, "").map_err(refused)?;
                            site = Some(s);
                            c3d::add_feature_line(doc, s, &line, points, layer).map_err(refused)?
                        }
                    };
                    changes.push((h, ChangeKind::Added));
                }
                crate::tf!("{} feature line(s) created.", changes.len()).into_owned()
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
