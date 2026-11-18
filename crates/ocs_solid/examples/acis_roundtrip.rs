//! Reads every 3DSOLID, SURFACE and REGION of the DWGs given (files or directories), lifts its
//! ACIS data into records, writes each record back and lifts it again, and reports how many
//! bodies survive each step with their volume.

use std::path::{Path, PathBuf};

use document::entities::acis::{SabReader, SatDocument};
use document::entities::AcisData;
use document::EntityType;

fn sat_of(data: &AcisData) -> Option<SatDocument> {
    if data.is_binary {
        SabReader::read(&data.sab_data).ok()
    } else {
        SatDocument::parse(&data.sat_data).ok()
    }
}

fn files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                files(&entry.path(), out);
            }
        }
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")) {
        out.push(path.to_path_buf());
    }
}

fn volume(brep: &solidcore_record::brep::Brep) -> Option<f64> {
    let (body, _) = match solidcore_ops::Body::from_record(brep, true) {
        Ok(found) => found,
        Err(e) => {
            if std::env::var("VERBOSE").is_ok() {
                eprintln!("  from_record: {e:?}");
                let mut kinds = std::collections::BTreeMap::new();
                for f in &brep.faces {
                    let k = format!("{:?}", f.surface).split([' ', '{', '(']).next().unwrap_or("").to_string();
                    *kinds.entry(format!("{k}{}", if f.nurbs.is_some() { "+nurbs" } else { "" })).or_insert(0) += 1;
                }
                for e in &brep.edges {
                    let k = format!("{:?}", e.curve).split([' ', '{', '(']).next().unwrap_or("").to_string();
                    *kinds.entry(format!("edge {k}{}", if e.nurbs.is_some() { "+nurbs" } else { "" })).or_insert(0) += 1;
                }
                eprintln!("   kinds {kinds:?}");
                for (i, f) in brep.faces.iter().enumerate() {
                    for s in [Some(&f.surface), f.nurbs.as_ref()].into_iter().flatten() {
                        if let solidcore_record::brep::Surface::Plane { u_axis, v_axis, .. } = s {
                            let d = u_axis[0] * v_axis[0] + u_axis[1] * v_axis[1] + u_axis[2] * v_axis[2];
                            let lu = (u_axis[0] * u_axis[0] + u_axis[1] * u_axis[1] + u_axis[2] * u_axis[2]).sqrt();
                            let lv = (v_axis[0] * v_axis[0] + v_axis[1] * v_axis[1] + v_axis[2] * v_axis[2]).sqrt();
                            if d.abs() > 3.0 * f64::EPSILON / (1.0 - 3.0 * f64::EPSILON) * lu * lv {
                                eprintln!("   face {i}: plane dot {d:e} |u| {lu} |v| {lv}");
                            }
                        }
                    }
                }
            }
            return None;
        }
    };
    match body.volume(1e-6) {
        Ok(v) => Some(v),
        Err(e) => {
            if std::env::var("VERBOSE").is_ok() {
                eprintln!("  volume: {e:?} (faces {}, regions {})", brep.faces.len(), brep.regions.len());
            }
            None
        }
    }
}

fn main() {
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        files(Path::new(&arg), &mut paths);
    }
    let (mut bodies, mut lifted, mut lossy, mut valid, mut written, mut relifted, mut same) = (0, 0, 0, 0, 0, 0, 0);
    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(doc) = document::DwgReader::from_stream(std::io::Cursor::new(bytes)).read() else { continue };
        for entity in doc.entities() {
            let data = match entity {
                EntityType::Solid3D(s) => &s.acis_data,
                EntityType::Surface(s) => &s.acis_data,
                EntityType::Region(s) => &s.acis_data,
                _ => continue,
            };
            let Some(sat) = sat_of(data) else { continue };
            let expected = sat.bodies().len();
            bodies += expected;
            let (lifts, loss) = ocs_solid::lift(&sat);
            if !loss.is_empty() {
                lossy += 1;
                if std::env::var("VERBOSE").is_ok() {
                    eprintln!("{}: loss {:?}", path.display(), loss);
                }
            }
            for one in lifts {
                lifted += 1;
                if solidcore_ops::record_check::validate(&one.brep).is_ok() {
                    valid += 1;
                }
                let before = volume(&one.brep);
                let mut out = SatDocument::new();
                if ocs_solid::append(&one.brep, &mut out).is_err() {
                    if std::env::var("VERBOSE").is_ok() {
                        eprintln!("{}: append failed", path.display());
                    }
                    continue;
                }
                written += 1;
                let Ok(reparsed) = SatDocument::parse(&out.to_sat_string()) else { continue };
                let (again, _) = ocs_solid::lift(&reparsed);
                let Some(again) = again.into_iter().next() else {
                    if std::env::var("VERBOSE").is_ok() {
                        eprintln!("{}: relift failed", path.display());
                    }
                    continue;
                };
                relifted += 1;
                let after = volume(&again.brep);
                let agree = match (before, after) {
                    (Some(a), Some(b)) => (a - b).abs() <= 1e-6 * a.abs().max(1.0),
                    (None, None) => again.brep.faces.len() == one.brep.faces.len(),
                    _ => false,
                };
                if agree {
                    same += 1;
                } else if std::env::var("VERBOSE").is_ok() {
                    eprintln!("{}: volume {before:?} -> {after:?}", path.display());
                }
            }
        }
    }
    println!("bodies {bodies} lifted {lifted} (documents with loss {lossy}) valid {valid} written {written} relifted {relifted} same {same}");
}
