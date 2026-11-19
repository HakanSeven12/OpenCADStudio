mod section;
pub(crate) mod source;

use std::collections::{BTreeMap, HashMap};

use ifc_lite_core::{decode_ifc_string, EntityDecoder, EntityScanner};
use ifc_lite_geometry::placement::resolve_object_placement;
use ifc_lite_processing::{process, ProcessingRequest};
use sha2::{Digest, Sha256};

use section::{plan_at, SourceSegment};

const CUT_LAYER: &str = "IFC-SNEDE";
const PROJECTION_LAYER: &str = "IFC-ONDER";
const TERRAIN_LAYER: &str = "IFC-TERREIN";
pub(crate) const LAYERS: [(&str, i16); 3] =
    [(CUT_LAYER, 35), (PROJECTION_LAYER, 13), (TERRAIN_LAYER, 13)];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Storey {
    pub id: u32,
    pub name: String,
    pub elevation: f64,
}

#[derive(Debug)]
pub struct PlanModel {
    meshes: Vec<Mesh>,
    anchor: [f64; 3],
    pub(crate) storeys: Vec<Storey>,
    project_global_id: Option<String>,
    revision: String,
    sources: BTreeMap<u32, IfcElement>,
}

#[derive(Debug)]
struct Mesh {
    source: u32,
    positions: Vec<f32>,
    indices: Vec<u32>,
}

impl Mesh {
    fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        let corner = |index: u32| {
            let i = index as usize * 3;
            let p = self.positions.get(i..i + 3)?;
            Some([p[0], p[1], p[2]])
        };
        self.indices
            .chunks_exact(3)
            .filter_map(move |t| Some([corner(t[0])?, corner(t[1])?, corner(t[2])?]))
    }
}

#[derive(Debug)]
struct IfcElement {
    global_id: Option<String>,
    ifc_type: String,
}

fn global_id(value: &str) -> Option<String> {
    (value.len() == 22
        && value.as_bytes()[0] <= b'3'
        && value.as_bytes()[0] >= b'0'
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$'))
    .then(|| value.to_owned())
}

fn read_project_and_storeys(
    content: &[u8],
    scale: f64,
    building_z: f64,
) -> (Option<String>, Vec<Storey>) {
    let mut scanner = EntityScanner::new(content);
    let mut decoder = EntityDecoder::new(content);
    let mut storeys = Vec::new();
    let mut projects = Vec::new();
    while let Some((id, keyword, start, end)) = scanner.next_entity() {
        if keyword.eq_ignore_ascii_case("IFCPROJECT") {
            projects.push(
                decoder
                    .decode_at(start, end)
                    .ok()
                    .and_then(|project| project.get_string(0).and_then(global_id)),
            );
        }
        if !keyword.eq_ignore_ascii_case("IFCBUILDINGSTOREY") {
            continue;
        }
        let Ok(storey) = decoder.decode_at(start, end) else {
            continue;
        };
        let text = |i| {
            storey
                .get_string(i)
                .map(|s| decode_ifc_string(s).trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let name = text(2)
            .or_else(|| text(7))
            .unwrap_or_else(|| format!("Storey #{id}"));
        let elevation = storey
            .get_ref(5)
            .and_then(|_| {
                resolve_object_placement(&storey, &mut decoder)
                    .ok()
                    .flatten()
            })
            .map(|m| m[(2, 3)] * scale)
            .or_else(|| storey.get_float(9).map(|z| building_z + z * scale));
        if let Some(elevation) = elevation.filter(|z| z.is_finite()) {
            storeys.push(Storey {
                id,
                name,
                elevation,
            });
        }
    }
    storeys.sort_by(|a, b| a.elevation.total_cmp(&b.elevation).then(a.id.cmp(&b.id)));
    let project = if projects.len() == 1 {
        projects.pop().flatten()
    } else {
        None
    };
    (project, storeys)
}

pub(crate) fn load_model(bytes: &[u8]) -> Result<PlanModel, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let start = bytes
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let content = &bytes[start..];
    if !content.starts_with(b"ISO-10303-21;") {
        return Err("Choose an IFC file with STEP content (.ifc).".into());
    }
    let request = ProcessingRequest::full_pass().instancing(false);
    #[cfg(not(target_arch = "wasm32"))]
    let result = ifc_lite_processing::run_on_geometry_pool(|| process(content, &request, &mut ()))
        .map_err(|error| format!("IFClite could not start its geometry workers: {error}"))?;
    #[cfg(target_arch = "wasm32")]
    let result = process(content, &request, &mut ());
    let world = result.frame.world_mapper();
    let anchor = result.frame.to_world([0.0; 3]);
    let scale = result.metadata.length_unit_scale.unwrap_or(1.0);
    let building_z = result
        .building_transform
        .as_ref()
        .and_then(|m| m.get(14))
        .copied()
        .unwrap_or(0.0);
    let mut meshes = Vec::new();
    let mut sources = BTreeMap::new();
    let mut decoder = EntityDecoder::new(content);

    for mesh in result.meshes {
        if mesh.indices.is_empty()
            || mesh.positions.is_empty()
            || matches!(
                mesh.ifc_type.to_ascii_uppercase().as_str(),
                "IFCSPACE"
                    | "IFCSPATIALZONE"
                    | "IFCOPENINGELEMENT"
                    | "IFCOPENINGSTANDARDCASE"
                    | "IFCVOIDINGFEATURE"
                    | "IFCVIRTUALELEMENT"
            )
        {
            continue;
        }
        let mut positions = Vec::with_capacity(mesh.positions.len());
        for point in mesh.positions.chunks_exact(3) {
            let point_world = world(std::array::from_fn(|k| point[k] as f64 + mesh.origin[k]));
            if !point_world.iter().all(|v| v.is_finite()) {
                return Err("IFClite returned an invalid geometry position.".into());
            }
            positions.extend((0..3).map(|k| (point_world[k] - anchor[k]) as f32));
        }
        sources
            .entry(mesh.express_id)
            .or_insert_with(|| IfcElement {
                global_id: mesh.global_id.as_deref().and_then(global_id).or_else(|| {
                    decoder
                        .decode_by_id(mesh.express_id)
                        .ok()
                        .and_then(|entity| entity.get_string(0).and_then(global_id))
                }),
                ifc_type: mesh.ifc_type.to_ascii_uppercase(),
            });
        meshes.push(Mesh {
            source: mesh.express_id,
            positions,
            indices: mesh.indices,
        });
    }
    if meshes.is_empty() {
        return Err(format!(
            "IFClite found no usable building geometry in {}.",
            result.metadata.schema_version
        ));
    }
    let (project_global_id, storeys) = read_project_and_storeys(content, scale, building_z);
    Ok(PlanModel {
        meshes,
        anchor,
        storeys,
        project_global_id,
        revision: format!("{:x}", Sha256::digest(bytes)),
        sources,
    })
}

#[derive(Debug)]
struct PlanLine {
    start: [f64; 3],
    end: [f64; 3],
    layer: &'static str,
    sources: Vec<u32>,
}

fn drawing_lines(
    groups: [Vec<SourceSegment>; 2],
    anchor: [f64; 3],
) -> Result<Vec<PlanLine>, String> {
    let mm = |[x, y]: [f32; 2]| {
        [
            (f64::from(x) + anchor[0]) * 1000.0,
            (f64::from(y) + anchor[1]) * 1000.0,
            0.0,
        ]
    };
    let mut lines: Vec<PlanLine> = Vec::new();
    let mut index: HashMap<_, usize> = HashMap::new();
    for (layer, segments) in [CUT_LAYER, PROJECTION_LAYER].into_iter().zip(groups) {
        for SourceSegment {
            points: [from, to],
            sources,
        } in segments
        {
            let snap = |p: [f32; 2]| p.map(|v| (f64::from(v) * 1_000_000.0).round() as i64);
            let ends = [snap(from), snap(to)];
            if ends[0] == ends[1] {
                continue;
            }
            if sources.is_empty() {
                return Err("A plan line has no IFC source.".into());
            }
            let line = index
                .entry((ends[0].min(ends[1]), ends[0].max(ends[1])))
                .or_insert(lines.len());
            if *line == lines.len() {
                lines.push(PlanLine {
                    start: mm(from),
                    end: mm(to),
                    layer,
                    sources,
                });
            } else {
                lines[*line].sources.extend(sources);
            }
        }
    }
    for line in &mut lines {
        line.sources.sort_unstable();
        line.sources.dedup();
    }
    if lines.is_empty() {
        return Err("No lines at this cut height or within the view depth.".into());
    }
    Ok(lines)
}

/// Convert the selected height range into editable millimetre CAD lines.
pub(crate) fn entities(
    model: &PlanModel,
    file: &str,
    cut_elevation: f64,
    depth: f64,
) -> Result<Vec<document::EntityType>, String> {
    use document::entities::Line;
    use document::types::Vector3;
    use document::xdata::{ExtendedDataRecord, XDataValue};

    let high = (cut_elevation - model.anchor[2]) as f32;
    let low = (cut_elevation - model.anchor[2] - depth) as f32;
    if !high.is_finite() || !low.is_finite() || low >= high {
        return Err("The cut height and viewing depth fall outside the supported range.".into());
    }
    let geometry = drawing_lines(plan_at(&model.meshes, low, high), model.anchor)?;
    let point = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
    let file = &file[..file.floor_char_boundary(250)];
    geometry
        .into_iter()
        .map(|segment| {
            let mut line = Line::from_points(point(segment.start), point(segment.end));
            let mut terrain_only = true;
            let mut data = ExtendedDataRecord::new(source::APPLICATION);
            for value in [
                "v1".to_owned(),
                format!("file={file}"),
                format!("sha256={}", model.revision),
                format!(
                    "project={}",
                    model.project_global_id.as_deref().unwrap_or("")
                ),
            ] {
                data.add_value(XDataValue::String(value));
            }
            let mut bytes = file.len() + 256;
            for id in segment.sources {
                let source = model
                    .sources
                    .get(&id)
                    .ok_or("Unknown IFC source element.")?;
                terrain_only &= source.ifc_type == "IFCSITE";
                let value = format!(
                    "ifc={id}|{}|{}",
                    source.ifc_type,
                    source.global_id.as_deref().unwrap_or("")
                );
                bytes += value.len() + 4;
                if value.len() > 255 || bytes > 16_000 {
                    return Err(
                        "A line has more IFC source data than DWG/DXF XDATA can store.".to_owned(),
                    );
                }
                data.add_value(XDataValue::String(value));
            }
            // A coincident building line remains visible if the terrain layer is hidden.
            line.common.layer = if terrain_only {
                TERRAIN_LAYER
            } else {
                segment.layer
            }
            .into();
            line.common.extended_data.add_record(data);
            Ok(document::EntityType::Line(line))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_geometry_retains_both_ifc_sources() {
        let model = PlanModel {
            meshes: [1, 2]
                .into_iter()
                .map(|source| Mesh {
                    source,
                    positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0],
                    indices: vec![0, 1, 2],
                })
                .collect(),
            anchor: [0.0; 3],
            storeys: Vec::new(),
            project_global_id: None,
            revision: "test".into(),
            sources: [1, 2]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        IfcElement {
                            ifc_type: "IFCWALL".into(),
                            global_id: Some(format!("0p4yrG6r55O8IYQGKv9vV{id}")),
                        },
                    )
                })
                .collect(),
        };
        let lines = entities(&model, "wall.ifc", 1.0, 1.5).unwrap();
        let cut = lines
            .iter()
            .find(|line| line.common().layer == CUT_LAYER)
            .unwrap();
        let provenance = source::read(cut.common()).unwrap();
        assert_eq!(provenance.file, Some("wall.ifc"));
        assert_eq!(
            provenance
                .elements
                .iter()
                .map(|e| e.step_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }
}
